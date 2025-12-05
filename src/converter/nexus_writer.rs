use crate::output::errors::NexusError;
use std::path::Path;

/// Write converted content to a Nexus playbook file
pub fn write_nexus_playbook(path: &Path, content: &str) -> Result<(), NexusError> {
    std::fs::write(path, content).map_err(|e| NexusError::Io {
        message: format!("Failed to write {}: {}", path.display(), e),
        path: Some(path.to_path_buf()),
    })
}

/// Generate output path for converted file
pub fn generate_output_path(source: &Path, output_dir: Option<&Path>) -> std::path::PathBuf {
    let filename = source.file_stem().unwrap_or_default();
    let new_name = format!("{}.nx.yml", filename.to_string_lossy());

    if let Some(dir) = output_dir {
        dir.join(&new_name)
    } else {
        source.with_file_name(&new_name)
    }
}
