use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run this script when dashboard sources change so the binary stays fresh.
    println!("cargo:rerun-if-changed=dashboard/src");
    println!("cargo:rerun-if-changed=dashboard/public");
    println!("cargo:rerun-if-changed=dashboard/package.json");
    println!("cargo:rerun-if-changed=dashboard/next.config.ts");
    println!("cargo:rerun-if-changed=dashboard/tsconfig.json");

    let out_dir = Path::new("dashboard/out");

    // If the output directory already exists this script was triggered by a
    // source-file change, so we must rebuild.  If it is absent (fresh checkout
    // or first run), we also build.  Either way: build.
    //
    // Developers who only touch Rust code won't hit this path because
    // cargo:rerun-if-changed prevents re-running when none of the listed files
    // change.
    if !out_dir.exists() {
        // First run: ensure npm packages are installed.
        let install = Command::new("npm")
            .args(["install", "--prefer-offline"])
            .current_dir("dashboard")
            .status();
        match install {
            Ok(s) if s.success() => {}
            Ok(s) => panic!("npm install exited with {s}"),
            Err(e) => panic!("npm not found ({e}). Install Node.js and re-run `cargo build`."),
        }
    }

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir("dashboard")
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("npm run build exited with {s}"),
        Err(e) => panic!("Failed to run npm ({e}). Ensure Node.js ≥ 18 is on PATH."),
    }
}
