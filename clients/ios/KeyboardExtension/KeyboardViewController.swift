// KeyboardViewController —— iOS 自定义键盘（docs/02 4.5.2）。
//
// 结构：候选条（横向 UICollectionView）+ 键盘面板（程序化布局，无 storyboard）。
// 按键流：字母/标点/退格/回车/Shift → X11 keysym → OpenKeySession.processKey；
// 组词态下由面板解析翻页/数字选择/空格选词，其余交 librime。
// 进程内直链 core（不经主 App IPC）；未开启 Full Access 时无网络与共享容器
// 访问——本版本词库与方案随 App 内置，输入功能完整。

import UIKit
import OpenKeyCore

final class KeyboardViewController: UIInputViewController {
    /// 引擎与会话：扩展进程内单例（首次按键时惰性创建）。
    private var engine: OpenKeyEngine?
    private var session: OpenKeySession?

    private let candidateBar = CandidateBarView()
    private lazy var keyPanel = KeyPanelView(onKey: { [weak self] key in
        self?.handle(key: key)
    })

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemGray6
        candidateBar.onSelect = { [weak self] index in self?.selectCandidate(index) }
        candidateBar.onPage = { [weak self] backward in self?.changePage(backward) }

        let stack = UIStackView(arrangedSubviews: [candidateBar, keyPanel])
        stack.axis = .vertical
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            stack.topAnchor.constraint(equalTo: view.topAnchor),
            stack.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        // 高度：候选条 44pt + 键盘主体（iPhone 竖屏约 216pt）。
        let height: CGFloat = 44 + 216
        if heightConstraint == nil {
            let constraint = view.heightAnchor.constraint(equalToConstant: height)
            constraint.priority = .defaultHigh
            constraint.isActive = true
            heightConstraint = constraint
        }
    }

    private var heightConstraint: NSLayoutConstraint?

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        ensureSession()
        refreshUI()
    }

    // MARK: - 会话

    private func ensureSession() {
        guard session == nil else { return }
        // 共享数据随 App 内置（Bundle.main/SharedData，打包脚本写入）。
        let support = FileManager.default.urls(for: .libraryDirectory,
                                               in: .userDomainMask).first!
        let shared = Bundle.main.bundleURL.appendingPathComponent("SharedData").path
        let user = support.appendingPathComponent("openkey").path
        try? FileManager.default.createDirectory(atPath: user, withIntermediateDirectories: true)

        if engine == nil {
            engine = OpenKeyEngine(sharedDataDir: shared,
                                   userDataDir: user,
                                   dataDbPath: user + "/openkey.db",
                                   componentsDir: user + "/components",
                                   minLogLevel: 2)
        }
        guard let engine, let newSession = engine.makeSession() else { return }
        // 设置页写入的方案（openkey.db 的 setting 条目）决定本会话方案。
        let store = OpenKeySettingsStore(engine: engine)
        _ = newSession.selectSchema(store.effectiveSchemaId())
        session = newSession
    }

    // MARK: - 按键

    /// 处理面板按键。
    fileprivate func handle(key: KeyPanelView.Key) {
        ensureSession()
        guard let session else { return }

        switch key {
        case .backspace:
            _ = session.processKey(keycode: X11Keysym.SpecialKey.backspace.keysym, mask: 0)
        case .returnKey:
            if session.context()?.isComposing == true {
                _ = session.commitComposition()
            }
            commitIfAny(session)
        case .shift:
            // Shift：临时切换 ascii_mode（英文模式）。
            let ascii = session.status()?.isAsciiMode ?? false
            _ = session.setOption("ascii_mode", !ascii)
        case .pageBackward, .pageForward:
            changePage(key == .pageBackward)
        case .digit(let index):
            guard session.context()?.isComposing == true else { return }
            selectCandidate(index)
        case .space:
            if let ctx = session.context(), ctx.isComposing {
                selectCandidate(max(ctx.highlightedCandidateIndex, 0))
            } else {
                _ = session.processKey(keycode: X11Keysym.keysym(for: " ") ?? 0x020, mask: 0)
                commitIfAny(session)
            }
        case .character(let character):
            guard let keysym = X11Keysym.keysym(for: Character(character.lowercased())) else {
                return
            }
            let mask = keyPanel.isUppercase ? X11Keysym.Mask.shift.rawValue : 0
            if !session.processKey(keycode: keysym, mask: mask) {
                // 未被输入法消费：英文模式下直接输出字符。
                if session.status()?.isAsciiMode == true {
                    textDocumentProxy.insertText(String(character))
                }
            }
        }
        commitIfAny(session)
        refreshUI()
    }

    private func commitIfAny(_ session: OpenKeySession) {
        if let text = session.commitText(), !text.isEmpty {
            textDocumentProxy.insertText(text)
        }
    }

    private func selectCandidate(_ index: Int) {
        guard let session else { return }
        _ = session.selectCandidate(index: index)
        commitIfAny(session)
    }

    private func changePage(_ backward: Bool) {
        guard let session else { return }
        _ = session.changePage(backward: backward)
    }

    private func refreshUI() {
        guard let session, let ctx = session.context() else { return }
        candidateBar.update(preedit: ctx.preedit, candidates: ctx.candidates)
        keyPanel.setAsciiMode(session.status()?.isAsciiMode ?? false)
    }
}

// MARK: - 键盘面板

/// 键盘面板：四行字母 + 底行（翻页/数字/空格/退格/回车/Shift）。程序化布局。
final class KeyPanelView: UIView {
    enum Key: Equatable {
        case character(Character)
        case space
        case backspace
        case returnKey
        case shift
        case pageBackward
        case pageForward
        case digit(Int)
    }

    private let onKey: (Key) -> Void
    private var asciiMode = false
    private(set) var isUppercase = true
    private var buttons: [(Key, UIButton)] = []

    private static let rows: [[Key]] = [
        [.character("q"), .character("w"), .character("e"), .character("r"), .character("t"),
         .character("y"), .character("u"), .character("i"), .character("o"), .character("p")],
        [.character("a"), .character("s"), .character("d"), .character("f"), .character("g"),
         .character("h"), .character("j"), .character("k"), .character("l")],
        [.shift, .character("z"), .character("x"), .character("c"), .character("v"),
         .character("b"), .character("n"), .character("m"), .backspace],
        [.pageBackward, .pageForward, .digit(1), .digit(2), .digit(3), .digit(4), .digit(5),
         .space, .returnKey],
    ]

    init(onKey: @escaping (Key) -> Void) {
        self.onKey = onKey
        super.init(frame: .zero)
        buildLayout()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) 未实现")
    }

    private func buildLayout() {
        let outer = UIStackView()
        outer.axis = .vertical
        outer.spacing = 6
        outer.distribution = .fillEqually
        outer.translatesAutoresizingMaskIntoConstraints = false
        addSubview(outer)
        NSLayoutConstraint.activate([
            outer.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3),
            outer.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
            outer.topAnchor.constraint(equalTo: topAnchor, constant: 3),
            outer.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -3),
        ])

        for row in Self.rows {
            let rowStack = UIStackView()
            rowStack.axis = .horizontal
            rowStack.spacing = 4
            rowStack.distribution = .fillEqually
            for key in row {
                let button = makeButton(for: key)
                buttons.append((key, button))
                rowStack.addArrangedSubview(button)
            }
            outer.addArrangedSubview(rowStack)
        }
        refreshAppearance()
    }

    private func makeButton(for key: Key) -> UIButton {
        let button = UIButton(type: .system)
        button.titleLabel?.font = .systemFont(ofSize: 20)
        button.layer.cornerRadius = 5
        button.addTarget(self, action: #selector(onTap(_:)), for: .touchUpInside)
        return button
    }

    private func refreshAppearance() {
        for (key, button) in buttons {
            button.setTitle(title(for: key), for: .normal)
            button.backgroundColor = background(for: key)
        }
    }

    private func title(for key: Key) -> String {
        switch key {
        case .character(let c):
            return isUppercase && !asciiMode ? String(c).uppercased() : String(c)
        case .space: return "空格"
        case .backspace: return "⌫"
        case .returnKey: return "换行"
        case .shift: return asciiMode ? "ABC" : "⇧"
        case .pageBackward: return "‹"
        case .pageForward: return "›"
        case .digit(let i): return "\(i)"
        }
    }

    private func background(for key: Key) -> UIColor {
        switch key {
        case .shift, .backspace, .returnKey, .pageBackward, .pageForward:
            return .systemGray3
        case .character:
            return asciiMode ? .systemGray : .systemBackground
        default:
            return .systemBackground
        }
    }

    func setAsciiMode(_ ascii: Bool) {
        guard asciiMode != ascii else { return }
        asciiMode = ascii
        refreshAppearance()
    }

    @objc private func onTap(_ sender: UIButton) {
        guard let entry = buttons.first(where: { $0.1 === sender }) else { return }
        let key = entry.0
        if case .shift = key {
            isUppercase.toggle()
            refreshAppearance()
        }
        onKey(key)
    }
}

// MARK: - 候选条

/// 候选条：预编辑 + 横向候选（点击选择）+ 翻页。
final class CandidateBarView: UIView {
    var onSelect: ((Int) -> Void)?
    var onPage: ((Bool) -> Void)?

    private let preeditLabel = UILabel()
    private let collection: UICollectionView
    private var candidates: [OpenKeyCandidate] = []

    override init(frame: CGRect) {
        let layout = UICollectionViewFlowLayout()
        layout.scrollDirection = .horizontal
        layout.itemSize = CGSize(width: 76, height: 36)
        collection = UICollectionView(frame: .zero, collectionViewLayout: layout)
        super.init(frame: frame)
        buildLayout()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) 未实现")
    }

    private func buildLayout() {
        backgroundColor = .secondarySystemBackground
        layer.cornerRadius = 6

        let pageBackward = UIButton(type: .system)
        pageBackward.setTitle("‹", for: .normal)
        pageBackward.addTarget(self, action: #selector(onPageBackward), for: .touchUpInside)

        let pageForward = UIButton(type: .system)
        pageForward.setTitle("›", for: .normal)
        pageForward.addTarget(self, action: #selector(onPageForward), for: .touchUpInside)

        preeditLabel.font = .systemFont(ofSize: 15)
        preeditLabel.textColor = .secondaryLabel
        preeditLabel.setContentHuggingPriority(.defaultLow, for: .horizontal)

        collection.backgroundColor = .clear
        collection.showsHorizontalScrollIndicator = false
        collection.register(CandidateCell.self, forCellWithReuseIdentifier: CandidateCell.reuseId)
        collection.dataSource = self
        collection.delegate = self

        let stack = UIStackView(arrangedSubviews: [pageBackward, preeditLabel, collection, pageForward])
        stack.axis = .horizontal
        stack.spacing = 4
        stack.alignment = .center
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    func update(preedit: String?, candidates: [OpenKeyCandidate]) {
        preeditLabel.text = preedit ?? ""
        self.candidates = candidates
        collection.reloadData()
    }

    @objc private func onPageBackward() { onPage?(true) }
    @objc private func onPageForward() { onPage?(false) }
}

extension CandidateBarView: UICollectionViewDataSource, UICollectionViewDelegate {
    func collectionView(_ collectionView: UICollectionView,
                        numberOfItemsInSection section: Int) -> Int {
        candidates.count
    }

    func collectionView(_ collectionView: UICollectionView,
                        cellForItemAt indexPath: IndexPath) -> UICollectionViewCell {
        let cell = collectionView.dequeueReusableCell(withReuseIdentifier: CandidateCell.reuseId,
                                                     for: indexPath)
        if let candidateCell = cell as? CandidateCell {
            let candidate = candidates[indexPath.item]
            candidateCell.configure(text: candidate.text, comment: candidate.comment)
        }
        return cell
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        onSelect?(indexPath.item)
    }
}

/// 候选项单元格。
final class CandidateCell: UICollectionViewCell {
    static let reuseId = "CandidateCell"
    private let label = UILabel()

    override init(frame: CGRect) {
        super.init(frame: frame)
        label.font = .systemFont(ofSize: 17)
        label.textAlignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
            label.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
            label.centerYAnchor.constraint(equalTo: contentView.centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) 未实现")
    }

    func configure(text: String, comment: String?) {
        label.text = comment.map { "\(text) \($0)" } ?? text
    }
}
