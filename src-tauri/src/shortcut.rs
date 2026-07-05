use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, OnceLock,
    },
    thread,
};

use tauri::{AppHandle, Emitter};
use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            VK_CAPITAL, VK_ESCAPE, VK_F1, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RETURN,
            VK_RMENU, VK_RSHIFT, VK_SPACE,
        },
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
            UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
            WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static TARGET_VK: AtomicU32 = AtomicU32::new(0);
static TARGET_KEY_DOWN: AtomicBool = AtomicBool::new(false);

pub struct ShortcutHandle {
    thread_id: u32,
    stopped: Arc<AtomicBool>,
}

impl ShortcutHandle {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn register(app: AppHandle, shortcut: &str) -> Result<ShortcutHandle, String> {
    let vk_code = shortcut_to_vk(shortcut)?;

    let _ = APP_HANDLE.set(app.clone());
    TARGET_VK.store(vk_code, Ordering::SeqCst);
    TARGET_KEY_DOWN.store(false, Ordering::SeqCst);
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_in_thread = stopped.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();

    thread::spawn(move || unsafe {
        let thread_id = windows::Win32::System::Threading::GetCurrentThreadId();
        let module = GetModuleHandleW(None)
            .ok()
            .map(|module| HINSTANCE(module.0));
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
            Ok(hook) => hook,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("全局快捷键监听失败：{error}")));
                return;
            }
        };

        let _ = ready_tx.send(Ok(thread_id));
        let mut msg = MSG::default();
        while !stopped_in_thread.load(Ordering::SeqCst) && GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        }
        let _ = UnhookWindowsHookEx(hook);
    });

    let thread_id = ready_rx
        .recv()
        .map_err(|_| "全局快捷键监听线程没有启动".to_string())??;

    Ok(ShortcutHandle { thread_id, stopped })
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        if event.vkCode == TARGET_VK.load(Ordering::SeqCst) {
            let message = wparam.0 as u32;
            if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
                let was_down = TARGET_KEY_DOWN.swap(true, Ordering::SeqCst);
                if !was_down {
                    if let Some(app) = APP_HANDLE.get() {
                        let _ = app.emit("global-shortcut", "toggle-recording");
                    }
                }
            } else if message == WM_KEYUP || message == WM_SYSKEYUP {
                TARGET_KEY_DOWN.store(false, Ordering::SeqCst);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

fn shortcut_to_vk(shortcut: &str) -> Result<u32, String> {
    let normalized = shortcut.trim();
    let vk = match normalized {
        "RightAlt" | "AltRight" | "右 Alt" | "右Alt" => VK_RMENU.0 as u32,
        "LeftAlt" | "AltLeft" => VK_LMENU.0 as u32,
        "RightControl" | "ControlRight" => VK_RCONTROL.0 as u32,
        "LeftControl" | "ControlLeft" => VK_LCONTROL.0 as u32,
        "RightShift" | "ShiftRight" => VK_RSHIFT.0 as u32,
        "LeftShift" | "ShiftLeft" => VK_LSHIFT.0 as u32,
        "Space" => VK_SPACE.0 as u32,
        "Enter" => VK_RETURN.0 as u32,
        "Escape" => VK_ESCAPE.0 as u32,
        "CapsLock" => VK_CAPITAL.0 as u32,
        key if key.starts_with('F') => {
            let number = key[1..]
                .parse::<u32>()
                .map_err(|_| format!("不支持的快捷键：{shortcut}"))?;
            if !(1..=24).contains(&number) {
                return Err(format!("不支持的快捷键：{shortcut}"));
            }
            VK_F1.0 as u32 + number - 1
        }
        key if key.starts_with("Key") && key.len() == 4 => key.as_bytes()[3] as u32,
        key if key.starts_with("Digit") && key.len() == 6 => key.as_bytes()[5] as u32,
        _ => return Err(format!("不支持的快捷键：{shortcut}")),
    };
    Ok(vk)
}
