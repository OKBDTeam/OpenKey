//! librime 输入会话封装（docs/02 3.1：按键/候选/上屏/方案/选项）。

#[cfg(feature = "librime")]
use std::os::raw::c_int;

#[cfg(feature = "librime")]
use crate::error::{cstr_to_string, to_cstring};
use crate::error::{Error, Result};
use crate::sys::*;

/// 一个候选字词。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 候选文本。
    pub text: String,
    /// 候选备注（如编码提示），可为空。
    pub comment: Option<String>,
}

/// 会话当前上下文（预编辑与候选）。
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// 预编辑文本（编码串），未组词时为 `None`。
    pub preedit: Option<String>,
    /// 光标位置（字符数）。
    pub cursor_pos: i32,
    /// 选择区起止（字符数，`sel_start < sel_end` 表示有选择）。
    pub sel_start: i32,
    /// 选择区结束（字符数）。
    pub sel_end: i32,
    /// 当前页候选。
    pub candidates: Vec<Candidate>,
    /// 页码（从 0 开始）。
    pub page_no: i32,
    /// 每页候选数。
    pub page_size: i32,
    /// 是否最后一页。
    pub is_last_page: bool,
    /// 当前高亮候选下标。
    pub highlighted: i32,
}

impl Context {
    /// 是否处于组词状态。
    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
    }

    /// 是否有候选菜单。
    pub fn has_menu(&self) -> bool {
        !self.candidates.is_empty()
    }
}

/// 会话状态快照。
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// 当前方案 ID。
    pub schema_id: String,
    /// 当前方案名。
    pub schema_name: String,
    /// 是否禁用。
    pub is_disabled: bool,
    /// 是否组词中。
    pub is_composing: bool,
    /// 是否英文（ascii）模式。
    pub is_ascii_mode: bool,
    /// 是否全角。
    pub is_full_shape: bool,
    /// 是否简体。
    pub is_simplified: bool,
    /// 是否繁体。
    pub is_traditional: bool,
    /// 是否西文标点。
    pub is_ascii_punct: bool,
}

/// librime 输入会话。`Drop` 时销毁会话。
pub struct Session {
    id: RimeSessionId,
}

impl Session {
    /// 创建一个新会话（需引擎已初始化）。
    pub fn create() -> Result<Self> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            // SAFETY: api 有效。
            let id = unsafe {
                (api.create_session
                    .ok_or(Error::Api("缺少 create_session"))?)()
            };
            if id == 0 {
                return Err(Error::Api("create_session 返回 0"));
            }
            Ok(Self { id })
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 会话 ID。
    pub fn id(&self) -> RimeSessionId {
        self.id
    }

    /// 处理一个按键。`keycode` 为 X11 keysym；`mask` 为 X11 修饰键掩码
    /// （Shift=1, Caps=2, Control=4, Alt/Mod1=8）。返回是否被 librime 消费。
    pub fn process_key(&self, keycode: i32, mask: i32) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            // SAFETY: api 与会话 ID 有效。
            let consumed = unsafe {
                (api.process_key.ok_or(Error::Api("缺少 process_key"))?)(
                    self.id,
                    keycode as c_int,
                    mask as c_int,
                )
            };
            Ok(consumed != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = (keycode, mask);
            Err(Error::NotLinked)
        }
    }

    /// 上屏当前预编辑（无候选选择时）。
    pub fn commit_composition(&self) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            // SAFETY: api 与会话 ID 有效。
            let ok = unsafe {
                (api.commit_composition
                    .ok_or(Error::Api("缺少 commit_composition"))?)(self.id)
            };
            Ok(ok != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 清除当前组词。
    pub fn clear_composition(&self) {
        #[cfg(feature = "librime")]
        {
            if let Ok(engine) = crate::engine::RimeEngine::global() {
                // SAFETY: api 与会话 ID 有效。
                unsafe {
                    if let Some(f) = engine.api().clear_composition {
                        f(self.id);
                    }
                }
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = self;
        }
    }

    /// 取当前上下文（预编辑 + 候选页）。
    pub fn context(&self) -> Result<Context> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let mut ctx = crate::rime_struct_init!(RimeContext, c);
            // SAFETY: ctx 已初始化；成功后立即提取并释放。
            unsafe {
                let get = api.get_context.ok_or(Error::Api("缺少 get_context"))?;
                if get(self.id, &mut ctx) == 0 {
                    return Ok(Context::default());
                }
                let mut out = Context::default();
                let comp = &ctx.composition;
                out.cursor_pos = comp.cursor_pos;
                out.sel_start = comp.sel_start;
                out.sel_end = comp.sel_end;
                if !comp.preedit.is_null() {
                    out.preedit = Some(cstr_to_string(comp.preedit));
                }
                let menu = &ctx.menu;
                out.page_no = menu.page_no;
                out.page_size = menu.page_size;
                out.is_last_page = menu.is_last_page != 0;
                out.highlighted = menu.highlighted_candidate_index;
                if menu.num_candidates > 0 && !menu.candidates.is_null() {
                    for i in 0..menu.num_candidates as usize {
                        let c = &*menu.candidates.add(i);
                        let text = cstr_to_string(c.text);
                        let comment = if c.comment.is_null() {
                            None
                        } else {
                            Some(cstr_to_string(c.comment))
                        };
                        out.candidates.push(Candidate { text, comment });
                    }
                }
                if let Some(free) = api.free_context {
                    free(&mut ctx);
                }
                Ok(out)
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 取待上屏文本（无则 `None`）。
    pub fn commit(&self) -> Result<Option<String>> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let mut commit = crate::rime_struct_init!(RimeCommit, c);
            // SAFETY: commit 已初始化；成功后立即提取并释放。
            unsafe {
                let get = api.get_commit.ok_or(Error::Api("缺少 get_commit"))?;
                if get(self.id, &mut commit) == 0 {
                    return Ok(None);
                }
                let text = cstr_to_string(commit.text);
                if let Some(free) = api.free_commit {
                    free(&mut commit);
                }
                Ok(Some(text))
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 取状态快照。
    pub fn status(&self) -> Result<Status> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let mut st = crate::rime_struct_init!(RimeStatus, s);
            // SAFETY: st 已初始化；成功后立即提取并释放。
            unsafe {
                let get = api.get_status.ok_or(Error::Api("缺少 get_status"))?;
                if get(self.id, &mut st) == 0 {
                    return Ok(Status::default());
                }
                let out = Status {
                    schema_id: cstr_to_string(st.schema_id),
                    schema_name: cstr_to_string(st.schema_name),
                    is_disabled: st.is_disabled != 0,
                    is_composing: st.is_composing != 0,
                    is_ascii_mode: st.is_ascii_mode != 0,
                    is_full_shape: st.is_full_shape != 0,
                    is_simplified: st.is_simplified != 0,
                    is_traditional: st.is_traditional != 0,
                    is_ascii_punct: st.is_ascii_punct != 0,
                };
                if let Some(free) = api.free_status {
                    free(&mut st);
                }
                Ok(out)
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 选择候选（`index` 为**当前页内**下标，与 `RimeContext.candidates` 的顺序
    /// 一致），整段选择完成时自动上屏。
    ///
    /// 两处 librime 语义需要注意：
    /// 1. `select_candidate` 的下标是**全列表**下标，而候选栏只展示当前页，
    ///    因此必须使用 `select_candidate_on_current_page`；
    /// 2. `_auto_commit` 未开启时，选择只是把该段标记为"已确认"并前移
    ///    composition（`ConcreteEngine::OnSelect`），**不会**写入提交缓冲。
    ///    因此整段选择后显式 `RimeCommitComposition` 冲刷（输入法语义：选中即
    ///    上屏）；部分选择（后续仍有输入）保持组词，不冲刷。
    pub fn select_candidate(&self, index: usize) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            // SAFETY: api 与会话 ID 有效。
            let ok = unsafe {
                (api.select_candidate_on_current_page
                    .ok_or(Error::Api("缺少 select_candidate_on_current_page"))?)(
                    self.id, index
                )
            };
            if ok != 0 {
                // 预编辑消失即表示输入已全部消费（整段选择）→ 冲刷提交缓冲。
                if self.context()?.preedit.is_none() {
                    self.commit_composition()?;
                }
            }
            Ok(ok != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = index;
            Err(Error::NotLinked)
        }
    }

    /// 翻页（`backward` 为上一页，否则下一页）。
    pub fn change_page(&self, backward: bool) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            // SAFETY: api 与会话 ID 有效。
            let ok = unsafe {
                (api.change_page.ok_or(Error::Api("缺少 change_page"))?)(
                    self.id,
                    if backward { TRUE } else { FALSE },
                )
            };
            Ok(ok != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = backward;
            Err(Error::NotLinked)
        }
    }

    /// 设置会话选项（如 `ascii_mode`）。
    pub fn set_option(&self, option: &str, value: bool) -> Result<()> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let c = to_cstring(option)?;
            // SAFETY: api 与会话 ID 有效；c 为有效 C 字符串。
            unsafe {
                (api.set_option.ok_or(Error::Api("缺少 set_option"))?)(
                    self.id,
                    c.as_ptr(),
                    if value { TRUE } else { FALSE },
                );
            }
            Ok(())
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = (option, value);
            Err(Error::NotLinked)
        }
    }

    /// 读取会话选项。
    pub fn get_option(&self, option: &str) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let c = to_cstring(option)?;
            // SAFETY: api 与会话 ID 有效；c 为有效 C 字符串。
            let value = unsafe {
                (api.get_option.ok_or(Error::Api("缺少 get_option"))?)(self.id, c.as_ptr())
            };
            Ok(value != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = option;
            Err(Error::NotLinked)
        }
    }

    /// 切换方案。
    pub fn select_schema(&self, schema_id: &str) -> Result<bool> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let c = to_cstring(schema_id)?;
            // SAFETY: api 与会话 ID 有效；c 为有效 C 字符串。
            let ok = unsafe {
                (api.select_schema.ok_or(Error::Api("缺少 select_schema"))?)(self.id, c.as_ptr())
            };
            Ok(ok != 0)
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = schema_id;
            Err(Error::NotLinked)
        }
    }

    /// 当前方案 ID。
    pub fn current_schema(&self) -> Result<String> {
        #[cfg(feature = "librime")]
        {
            let api = crate::engine::RimeEngine::global()?.api();
            let mut buf = vec![0u8; 256];
            // SAFETY: buf 容量随调用传入。
            unsafe {
                let f = api
                    .get_current_schema
                    .ok_or(Error::Api("缺少 get_current_schema"))?;
                if f(
                    self.id,
                    buf.as_mut_ptr() as *mut std::os::raw::c_char,
                    buf.len(),
                ) == 0
                {
                    return Ok(String::new());
                }
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
        }
        #[cfg(not(feature = "librime"))]
        {
            Err(Error::NotLinked)
        }
    }

    /// 当前原始输入串（编码）。
    pub fn input(&self) -> String {
        #[cfg(feature = "librime")]
        {
            if let Ok(engine) = crate::engine::RimeEngine::global() {
                // SAFETY: 返回会话内静态缓冲区指针。
                return unsafe {
                    cstr_to_string(
                        engine
                            .api()
                            .get_input
                            .map(|f| f(self.id))
                            .unwrap_or(std::ptr::null()),
                    )
                };
            }
            String::new()
        }
        #[cfg(not(feature = "librime"))]
        {
            String::new()
        }
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("id", &self.id).finish()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(feature = "librime")]
        {
            if let Ok(engine) = crate::engine::RimeEngine::global() {
                // SAFETY: api 与会话 ID 有效。
                unsafe {
                    if let Some(f) = engine.api().destroy_session {
                        f(self.id);
                    }
                }
            }
        }
        #[cfg(not(feature = "librime"))]
        {
            let _ = self;
        }
    }
}
