//! Script de build: incorpora o ícone `assets/icone.ico` no `TFM_GCODE.exe`.
//!
//! Só roda no Windows. É tolerante: se o `rc.exe` do SDK não estiver
//! disponível, emite um aviso e segue — nunca falha o build.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icone.ico");
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icone.ico");
    if !icon.is_file() {
        println!("cargo:warning=tfm-app: assets/icone.ico não encontrado; .exe sem ícone");
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&icon.to_string_lossy());
    res.set("ProductName", "TFM G-Code Generator");
    res.set(
        "FileDescription",
        "TFM G-Code Generator - Cladding & Hardfacing",
    );
    res.set("CompanyName", "TFM Usinagem & Manutenção Industrial LTDA");
    if let Err(err) = res.compile() {
        println!("cargo:warning=tfm-app: não foi possível incorporar o ícone no .exe ({err}); seguindo sem ele");
    }
}

#[cfg(not(windows))]
fn embed_icon() {}
