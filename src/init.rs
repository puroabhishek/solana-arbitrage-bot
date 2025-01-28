pub fn initialize_directories() -> std::io::Result<()> {
    let required_dirs = vec!["config", "logs", "data"];
    
    for dir in required_dirs {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}