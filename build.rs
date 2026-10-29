use std::{env, fs, path::Path};

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");

    // Give the Windows .exe its icon and version details, with the version
    // taken from Cargo.toml so it is only ever written in one place.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env::var("CARGO_PKG_VERSION").unwrap();
        let commas = ["MAJOR", "MINOR", "PATCH"]
            .map(|part| env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap())
            .join(",")
            + ",0";
        let icon = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/icons/aisleriot.ico");
        let rc = fs::read_to_string("assets/icons/aisleriot.rc.in")
            .unwrap()
            .replace("@VERSION_COMMAS@", &commas)
            .replace("@VERSION@", &version)
            .replace("@ICON@", &icon.display().to_string().replace('\\', "/"));
        let rc_path = Path::new(&env::var("OUT_DIR").unwrap()).join("rusty-solitaire.rc");
        fs::write(&rc_path, rc).unwrap();
        embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().unwrap();
        println!("cargo:rerun-if-changed=assets/icons/aisleriot.rc.in");
    }

    // Embed every card theme in the binary so the app ships as a single file
    // on all three platforms.
    let mut themes: Vec<String> = fs::read_dir("assets/cards")
        .expect("assets/cards missing")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".svgz"))
        .collect();
    themes.sort_by_key(|name| name.to_lowercase());

    let mut code = String::from("pub static CARD_THEMES: &[(&str, &[u8])] = &[\n");
    for name in &themes {
        code.push_str(&format!(
            "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/cards/{name}\"))),\n"
        ));
    }
    code.push_str("];\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("card_themes.rs"), code).unwrap();
    println!("cargo:rerun-if-changed=assets/cards");

    // The original Aisleriot game rule files, run by src/scheme.
    let mut games: Vec<String> = fs::read_dir("assets/games")
        .expect("assets/games missing")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".scm"))
        .collect();
    games.sort();
    let mut code = String::from("pub static GAME_FILES: &[(&str, &str)] = &[\n");
    for name in &games {
        code.push_str(&format!(
            "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/games/{name}\"))),\n"
        ));
    }
    code.push_str("];\n");
    fs::write(Path::new(&out_dir).join("game_files.rs"), code).unwrap();
    println!("cargo:rerun-if-changed=assets/games");
}
