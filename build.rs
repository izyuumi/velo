use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/package.json");

    let frontend_dir = std::path::Path::new("frontend");
    if !frontend_dir.exists() {
        return;
    }

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(frontend_dir)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("Frontend build failed with status: {}", s);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run npm build: {}", e);
            std::process::exit(1);
        }
    }
}

