import AppKit
import OpenKeyCore

/// 自绘候选条（NSPanel，不激活应用）。
///
/// docs/02 4.2.1：不使用系统 IMKCandidates（macOS 26 渲染缺陷，见调研报告 2.2），
/// 预编辑与候选都在本面板内绘制。
///
/// Phase 1 简化：
/// - 定位用鼠标位置兜底（TODO：接 IMKTextInput 的 caret 矩形）；
/// - 点击候选/翻页经闭包回调给 InputController，面板自身不做业务；
/// - 候选按钮用固定大小按钮池（按页候选数上限 10 预创建），只改标题与显隐。
final class CandidatePanel {
    /// 点击第 index 个候选。
    var onSelect: ((Int) -> Void)?
    /// 翻页（true 为下一页）。
    var onPage: ((Bool) -> Void)?

    private static let maxVisibleCandidates = 10
    private static let panelHeight: CGFloat = 36

    private let panel: NSPanel
    private let preeditLabel = NSTextField(labelWithString: "")
    private let candidatesStack = NSStackView()
    private let previousButton = NSButton(title: "‹", target: nil, action: nil)
    private let nextButton = NSButton(title: "›", target: nil, action: nil)
    private var candidateButtons: [NSButton] = []

    init() {
        let initialSize = NSSize(width: 480, height: Self.panelHeight)
        panel = NSPanel(
            contentRect: NSRect(origin: .zero, size: initialSize),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .popUpMenu
        panel.isFloatingPanel = true
        panel.hidesOnDeactivateKey = false
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true

        // 毛玻璃背景容器（borderless 面板自绘外观）。
        let effectView = NSVisualEffectView(frame: NSRect(origin: .zero, size: initialSize))
        effectView.material = .popover
        effectView.blendingMode = .behindWindow
        effectView.state = .active
        effectView.wantsLayer = true
        effectView.layer?.cornerRadius = 8
        effectView.layer?.masksToBounds = true
        // 面板改宽度时内容视图跟随。
        effectView.autoresizingMask = [.width, .height]

        preeditLabel.font = NSFont.systemFont(ofSize: 13)
        preeditLabel.textColor = .secondaryLabelColor
        preeditLabel.lineBreakMode = .byTruncatingTail
        preeditLabel.setContentCompressionResistancePriority(.required, for: .horizontal)

        candidatesStack.orientation = .horizontal
        candidatesStack.spacing = 4
        candidatesStack.alignment = .centerY
        candidatesStack.distribution = .fill

        for button in [previousButton, nextButton] {
            button.isBordered = false
            button.font = NSFont.systemFont(ofSize: 13)
            button.target = self
            button.action = #selector(pageButtonClicked(_:))
        }
        previousButton.tag = 0 // 上一页
        nextButton.tag = 1     // 下一页

        // 候选按钮池：预创建固定数量，运行时只改标题/显隐（隐藏项不参与栈视图布局）。
        for _ in 0..<Self.maxVisibleCandidates {
            let button = NSButton(title: "", target: self, action: #selector(candidateClicked(_:)))
            button.isBordered = false
            button.font = NSFont.systemFont(ofSize: 14)
            button.isHidden = true
            candidatesStack.addArrangedSubview(button)
            candidateButtons.append(button)
        }

        let row = NSStackView(views: [preeditLabel, candidatesStack, previousButton, nextButton])
        row.orientation = .horizontal
        row.spacing = 8
        row.alignment = .centerY
        row.edgeInsets = NSEdgeInsets(top: 4, left: 10, bottom: 4, right: 10)

        effectView.addSubview(row)
        row.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: effectView.leadingAnchor),
            row.trailingAnchor.constraint(equalTo: effectView.trailingAnchor),
            row.topAnchor.constraint(equalTo: effectView.topAnchor),
            row.bottomAnchor.constraint(equalTo: effectView.bottomAnchor),
        ])

        panel.contentView = effectView
    }

    /// 用最新上下文刷新面板内容。
    func update(context: OpenKeyContext) {
        if let preedit = context.preedit, !preedit.isEmpty {
            preeditLabel.stringValue = preedit
            preeditLabel.isHidden = false
        } else {
            preeditLabel.isHidden = true
        }

        // 候选按钮池刷新（Phase 1：候选只展示正文，备注 comment 留待 Phase 2）。
        var estimatedWidth: CGFloat = 0
        for (index, button) in candidateButtons.enumerated() {
            guard index < context.candidates.count else {
                button.isHidden = true
                continue
            }
            let candidate = context.candidates[index]
            let highlighted = index == context.highlightedCandidateIndex
            button.title = candidate.text
            button.tag = index
            button.isHidden = false
            button.font = highlighted ? NSFont.boldSystemFont(ofSize: 14) : NSFont.systemFont(ofSize: 14)
            button.contentTintColor = highlighted ? .controlAccentColor : .labelColor
            // 中文候选按全宽字宽估算按钮宽度（仅用于面板总宽估算，非精确布局）。
            estimatedWidth += CGFloat(candidate.text.count) * 15 + 20
        }
        candidatesStack.isHidden = context.candidates.isEmpty

        let hasMultiplePages = context.pageSize > 0 && !(context.isLastPage && context.pageNo == 0)
        previousButton.isHidden = !hasMultiplePages
        nextButton.isHidden = !hasMultiplePages
        previousButton.isEnabled = context.pageNo > 0
        nextButton.isEnabled = !context.isLastPage

        // 依据内容估算面板宽度（预编辑 + 候选 + 翻页按钮 + 内边距），并限制范围。
        let preeditWidth = preeditLabel.isHidden ? 0 : CGFloat((context.preedit ?? "").count) * 13 + 8
        let width = min(max(estimatedWidth + preeditWidth + 120, 200), 640)
        var frame = panel.frame
        frame.size = NSSize(width: width, height: Self.panelHeight)
        panel.setFrame(frame, display: false)
    }

    /// 在指定屏幕点附近显示（Phase 1：鼠标位置兜底定位）。
    func show(near point: NSPoint) {
        let screen = NSScreen.screens.first { NSMouseInRect(point, $0.frame, false) } ?? NSScreen.main
        guard let screen else {
            panel.orderFrontRegardless()
            return
        }
        var origin = NSPoint(x: point.x + 8, y: point.y - panel.frame.height - 4)
        origin.x = min(origin.x, screen.frame.maxX - panel.frame.width - 8)
        origin.y = max(origin.y, screen.frame.minY + 8)
        panel.setFrameOrigin(origin)
        panel.orderFrontRegardless()
    }

    func hide() {
        panel.orderOut(nil)
    }

    // MARK: - 回调

    @objc
    private func candidateClicked(_ sender: NSButton) {
        onSelect?(sender.tag)
    }

    @objc
    private func pageButtonClicked(_ sender: NSButton) {
        onPage?(sender.tag == 1)
    }
}
