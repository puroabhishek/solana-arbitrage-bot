pub fn ensure_file_exists(path: &str) -> std::io::Result<()> {
    if !Path::new(path).exists() {
        fs::create_dir_all(Path::new(path).parent().unwrap())?;
        fs::File::create(path)?;
    }
    Ok(())
}