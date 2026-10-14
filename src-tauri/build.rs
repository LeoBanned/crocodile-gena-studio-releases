fn main() {
    embed_optional_hypervisor_resources();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("PROFILE").as_deref() != Ok("release")
    {
        tauri_build::build();
        return;
    }
    let manifest = r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to run build script");
}

// Source distributions intentionally omit these third-party launch helpers.
// Generate includes only for files actually present, never zero-byte resource files.
fn embed_optional_hypervisor_resources() {
    use std::{env, fs, path::PathBuf};
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let directory = root.join("resources/hypervisor");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut generated = String::new();
    for (name, constant, is_text) in [
        ("HV-Launcher.bat", "EMBEDDED_HV_BATCH", true),
        ("HV-PlugNPlay.bat", "EMBEDDED_PLUG_N_PLAY_BATCH", true),
        ("kvc.exe", "EMBEDDED_KVC", false),
        ("drvloader.exe", "EMBEDDED_DRVLOADER", false),
    ] {
        let file = directory.join(name);
        println!("cargo:rerun-if-changed={}", file.display());
        let available = match fs::metadata(&file) {
            Ok(metadata) => metadata.is_file() && metadata.len() > 0,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => panic!(
                "Cannot inspect optional resource {}: {error}",
                file.display()
            ),
        };
        let (ty, expression) = if is_text {
            (
                "&str",
                if available {
                    format!("include_str!({:?})", file)
                } else {
                    "\"\"".into()
                },
            )
        } else {
            (
                "&[u8]",
                if available {
                    format!("include_bytes!({:?})", file)
                } else {
                    "&[]".into()
                },
            )
        };
        generated.push_str(&format!("const {constant}: {ty} = {expression};\n"));
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("hypervisor_resources.rs");
    fs::write(output, generated).expect("Cannot generate optional hypervisor resource constants");
}
