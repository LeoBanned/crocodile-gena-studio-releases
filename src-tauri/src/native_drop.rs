#[tauri::command]
pub(crate) async fn enable_file_drop(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let target = window.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = sender.send(windows_drop::register(&target));
            })
            .map_err(|error| error.to_string())?;
        tauri::async_runtime::spawn_blocking(move || {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|error| format!("Не удалось включить перетаскивание: {error}"))?
        })
        .await
        .map_err(|error| error.to_string())??;
    }
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
}

#[cfg(windows)]
mod windows_drop {
    use serde::Serialize;
    use tauri::{Emitter, WebviewWindow};
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, WPARAM},
            System::Ole::RevokeDragDrop,
            UI::{
                Shell::{
                    DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW,
                    GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass, HDROP,
                },
                WindowsAndMessaging::{
                    ChangeWindowMessageFilterEx, EnumChildWindows, KillTimer, SetTimer,
                    MSGFLT_ALLOW, WM_DROPFILES, WM_NCDESTROY, WM_TIMER,
                },
            },
        },
    };

    const SUBCLASS_ID: usize = 0x43475344;
    // Explorer transfers the HDROP memory with this Shell message before WM_DROPFILES.
    const WM_COPYGLOBALDATA: u32 = 0x0049;

    #[derive(Clone, Serialize)]
    struct DropPayload {
        paths: Vec<String>,
    }

    pub(super) fn register(window: &WebviewWindow) -> Result<(), String> {
        let handle = window.hwnd().map_err(|error| error.to_string())?;
        let hwnd = HWND(handle.0);
        unsafe {
            register_handle(hwnd, window)?;
            // WebView2 can recreate child HWNDs. Reapply the Shell receiver to new children.
            SetTimer(Some(hwnd), SUBCLASS_ID, 1500, None);
            // The cursor normally lands on WebView2's child HWND, not the top-level HWND.
            let _ = EnumChildWindows(
                Some(hwnd),
                Some(register_child),
                LPARAM(window as *const _ as isize),
            );
        }
        Ok(())
    }

    unsafe extern "system" fn register_child(hwnd: HWND, data: LPARAM) -> BOOL {
        let window = &*(data.0 as *const WebviewWindow);
        let _ = register_handle(hwnd, window);
        BOOL(1)
    }

    unsafe fn register_handle(hwnd: HWND, window: &WebviewWindow) -> Result<(), String> {
        // Allow only file-drop messages, preserving the process's other UIPI restrictions.
        ChangeWindowMessageFilterEx(hwnd, WM_DROPFILES, MSGFLT_ALLOW, None)
            .map_err(|error| error.to_string())?;
        ChangeWindowMessageFilterEx(hwnd, WM_COPYGLOBALDATA, MSGFLT_ALLOW, None)
            .map_err(|error| error.to_string())?;
        if !GetWindowSubclass(hwnd, Some(drop_proc), SUBCLASS_ID, None).as_bool() {
            let context = Box::into_raw(Box::new(window.clone()));
            if !SetWindowSubclass(hwnd, Some(drop_proc), SUBCLASS_ID, context as usize).as_bool() {
                drop(Box::from_raw(context));
                return Err("Could not register file-drop window callback".into());
            }
        }
        // OLE drag targets take precedence over WM_DROPFILES, and refuse drops across UAC
        // integrity levels. Remove Wry/WebView2's OLE receiver before accepting Shell drops.
        // WebView2 external browser drops are already disabled by Wry, avoiding ZIP navigation.
        use_shell_drop(hwnd);
        Ok(())
    }

    unsafe fn use_shell_drop(hwnd: HWND) {
        let _ = RevokeDragDrop(hwnd);
        DragAcceptFiles(hwnd, true);
    }

    unsafe extern "system" fn drop_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        data: usize,
    ) -> LRESULT {
        if message == WM_TIMER && wparam.0 == SUBCLASS_ID {
            let window = &*(data as *const WebviewWindow);
            let _ = EnumChildWindows(
                Some(hwnd),
                Some(register_child),
                LPARAM(window as *const _ as isize),
            );
            return LRESULT(0);
        }
        if message == WM_DROPFILES {
            let handle = HDROP(wparam.0 as *mut _);
            let paths = read_drop(handle);
            DragFinish(handle);
            if !paths.is_empty() {
                let window = &*(data as *const WebviewWindow);
                let _ = window.emit("cgs-file-drop", DropPayload { paths });
            }
            return LRESULT(0);
        }
        if message == WM_NCDESTROY {
            let _ = KillTimer(Some(hwnd), SUBCLASS_ID);
            let _ = RemoveWindowSubclass(hwnd, Some(drop_proc), subclass_id);
            drop(Box::from_raw(data as *mut WebviewWindow));
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    unsafe fn read_drop(handle: HDROP) -> Vec<String> {
        let count = DragQueryFileW(handle, u32::MAX, None).min(1024);
        let mut paths = Vec::new();
        for index in 0..count {
            let length = DragQueryFileW(handle, index, None);
            if length == 0 || length > 32_768 {
                continue;
            }
            let mut buffer = vec![0u16; length as usize + 1];
            let written = DragQueryFileW(handle, index, Some(&mut buffer));
            if written > 0 {
                let path = String::from_utf16_lossy(&buffer[..written as usize]);
                if std::path::Path::new(&path).exists() {
                    paths.push(path);
                }
            }
        }
        paths
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::core::{self as windows_core, implement, w};
        use windows::Win32::{
            Foundation::{DRAGDROP_E_ALREADYREGISTERED, POINTL},
            System::Com::IDataObject,
            System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
            System::Ole::{
                IDropTarget, IDropTarget_Impl, OleInitialize, OleUninitialize, RegisterDragDrop,
                DROPEFFECT,
            },
            System::SystemServices::MODIFIERKEYS_FLAGS,
            UI::Shell::DROPFILES,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetWindowLongPtrW, GWL_EXSTYLE, WINDOW_EX_STYLE,
                WINDOW_STYLE, WS_EX_ACCEPTFILES,
            },
        };

        #[implement(IDropTarget)]
        struct BrowserDropTarget;
        #[allow(non_snake_case)]
        impl IDropTarget_Impl for BrowserDropTarget_Impl {
            fn DragEnter(
                &self,
                _: windows_core::Ref<'_, IDataObject>,
                _: MODIFIERKEYS_FLAGS,
                _: &POINTL,
                _: *mut DROPEFFECT,
            ) -> windows_core::Result<()> {
                Ok(())
            }
            fn DragOver(
                &self,
                _: MODIFIERKEYS_FLAGS,
                _: &POINTL,
                _: *mut DROPEFFECT,
            ) -> windows_core::Result<()> {
                Ok(())
            }
            fn DragLeave(&self) -> windows_core::Result<()> {
                Ok(())
            }
            fn Drop(
                &self,
                _: windows_core::Ref<'_, IDataObject>,
                _: MODIFIERKEYS_FLAGS,
                _: &POINTL,
                _: *mut DROPEFFECT,
            ) -> windows_core::Result<()> {
                Ok(())
            }
        }

        #[test]
        fn shell_receiver_removes_a_real_registered_browser_ole_target() {
            unsafe {
                OleInitialize(None).unwrap();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("CGS drop regression"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let target: IDropTarget = BrowserDropTarget.into();
                RegisterDragDrop(hwnd, &target).unwrap();
                assert_eq!(
                    RegisterDragDrop(hwnd, &target).unwrap_err().code(),
                    DRAGDROP_E_ALREADYREGISTERED
                );
                use_shell_drop(hwnd);
                assert_ne!(
                    GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_ACCEPTFILES.0 as isize,
                    0
                );
                // A new registration succeeds only if the conflicting target was actually revoked.
                RegisterDragDrop(hwnd, &target).unwrap();
                RevokeDragDrop(hwnd).unwrap();
                DestroyWindow(hwnd).unwrap();
                OleUninitialize();
            }
        }

        #[test]
        fn explorer_drop_preserves_unicode_spaces_and_multiple_paths() {
            let root = std::env::temp_dir().join(format!("cgs-drop-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let zip = root.join("Моя игра.zip");
            let folder = root.join("Папка игры");
            std::fs::write(&zip, b"zip fixture").unwrap();
            std::fs::create_dir_all(&folder).unwrap();
            let paths = [zip, folder, root.join("missing.zip")];
            let mut names = Vec::<u16>::new();
            for path in &paths {
                names.extend(path.to_str().unwrap().encode_utf16());
                names.push(0);
            }
            names.push(0);
            unsafe {
                let offset = std::mem::size_of::<DROPFILES>();
                let memory =
                    GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, offset + names.len() * 2).unwrap();
                let base = GlobalLock(memory);
                assert!(!base.is_null());
                let header = base.cast::<DROPFILES>();
                (*header).pFiles = offset as u32;
                (*header).fWide = BOOL(1);
                std::ptr::copy_nonoverlapping(
                    names.as_ptr(),
                    base.cast::<u8>().add(offset).cast(),
                    names.len(),
                );
                let _ = GlobalUnlock(memory);
                let handle = HDROP(memory.0);
                let received = read_drop(handle);
                DragFinish(handle);
                assert_eq!(
                    received,
                    paths[..2]
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
