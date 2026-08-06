use std::fs;
use std::path::{Path, PathBuf};

mod analysis;
mod data;
mod export;
mod html;
mod svg;

pub(crate) fn generate(
    results_directory: &Path,
    output_directory: &Path,
    repo_base: &str,
    catalog_directory: Option<&Path>,
) -> Result<usize, String> {
    let dataset = data::load(results_directory, catalog_directory)?;
    let repo_base = repo_base.trim_end_matches('/');

    fs::create_dir_all(output_directory).map_err(|error| {
        format!(
            "failed to create site directory {}: {error}",
            output_directory.display()
        )
    })?;
    write_site_file(
        output_directory.join("index.html"),
        &html::render_index(&dataset, repo_base),
    )?;
    write_site_file(
        output_directory.join("outcomes.html"),
        &html::render_outcomes(&dataset),
    )?;
    write_site_file(
        output_directory.join("appendix.html"),
        &html::render_appendix_page(&dataset, repo_base),
    )?;
    write_site_file(
        output_directory.join("submit.html"),
        &html::render_submit(repo_base),
    )?;
    write_site_file(
        output_directory.join("results.json"),
        &export::render_json(&dataset)?,
    )?;
    write_site_file(
        output_directory.join("results.csv"),
        &export::render_csv(&dataset),
    )?;
    write_site_file(output_directory.join("style.css"), html::STYLE)?;
    write_site_file(output_directory.join("site.js"), html::SCRIPT)?;
    Ok(dataset.rows.len())
}

fn write_site_file(path: PathBuf, contents: &str) -> Result<(), String> {
    fs::write(&path, contents)
        .map_err(|error| format!("failed to write site file {}: {error}", path.display()))
}
