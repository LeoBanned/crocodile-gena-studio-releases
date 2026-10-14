/// The Explorer-style Windows folder picker. Shared by every folder action.
pub(crate) fn pick_folder(title: &str, owner: isize) -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        use windows::{
            core::{HRESULT, HSTRING},
            Win32::{
                Foundation::HWND,
                System::Com::{
                    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
                },
                UI::Shell::{
                    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                    FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
                },
            },
        };
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let result = unsafe {
            (|| -> windows::core::Result<Option<String>> {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
                let _apartment = Apartment;
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
                let options = dialog.GetOptions()?;
                dialog.SetOptions(
                    options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                )?;
                dialog.SetTitle(&HSTRING::from(title))?;
                if let Err(error) = dialog.Show(Some(HWND(owner as *mut _))) {
                    if error.code() == HRESULT(0x800704c7u32 as i32) {
                        return Ok(None);
                    }
                    return Err(error);
                }
                let item = dialog.GetResult()?;
                let selected = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                let path = selected.to_string();
                CoTaskMemFree(Some(selected.0.cast()));
                Ok(Some(path?))
            })()
        };
        result.map_err(|error| format!("Не удалось открыть выбор папки: {error}"))
    }
    #[cfg(not(windows))]
    {
        let _ = owner;
        Ok(rfd::FileDialog::new()
            .set_title(title)
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned()))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn pick_files(title: &str, extensions: &[&str]) -> Vec<String> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Поддерживаемые файлы", extensions)
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(target_os = "linux")]
pub(crate) fn save_file(title: &str, file_name: &str, extension: &str) -> Option<String> {
    rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(file_name)
        .add_filter(extension.to_ascii_uppercase(), &[extension])
        .save_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
pub(crate) fn pick_file(title: &str, extensions: &[&str]) -> Option<String> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Поддерживаемые файлы", extensions)
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}
