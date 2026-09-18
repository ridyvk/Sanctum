use crate::domain::{
    Attachment, BlockCitationRecord, CitationRecord, HypothesisBlock, PortableExportRecord,
};
use crate::error::{Result, SanctumError};
use crate::{
    atomic_write, rename_noreplace, sha256_file, sync_directory, sync_file,
    sync_tree_directories, utc_now, Vault,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    path: String,
    sha256: String,
    byte_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportManifest {
    format_version: u32,
    vault_id: String,
    vault_name: String,
    source_revision: i64,
    exported_at: String,
    block_count: usize,
    attachment_count: usize,
    citation_count: usize,
    files: Vec<ExportFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CitationExport {
    citation: CitationRecord,
    block_ids: Vec<String>,
}

impl Vault {
    pub fn export_portable_to(&self, parent: impl AsRef<Path>) -> Result<PortableExportRecord> {
        let parent = parent.as_ref();
        if !parent.is_dir() {
            return Err(SanctumError::InvalidInput(
                "export destination must be an existing directory".into(),
            ));
        }
        let summary = self.summary()?;
        let exported_at = utc_now();
        let safe_name = safe_component(&summary.name);
        let stamp = exported_at
            .chars()
            .filter(char::is_ascii_digit)
            .take(14)
            .collect::<String>();
        let destination = unique_destination(parent, &format!("Sanctum-Export-{safe_name}-{stamp}"));
        self.export_portable(&destination)
    }

    pub fn export_portable(&self, destination: impl AsRef<Path>) -> Result<PortableExportRecord> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| SanctumError::InvalidInput("export path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let staging = parent.join(format!(
            ".{}.exporting-{}",
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("sanctum-export"),
            Uuid::new_v4()
        ));
        fs::create_dir(&staging)?;

        let result = self.build_portable_export(&staging, destination);
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                let quarantine = parent.join(format!(
                    ".sanctum-failed-export-{}",
                    Uuid::new_v4()
                ));
                let _ = rename_noreplace(&staging, &quarantine);
                return Err(error);
            }
        };
        sync_tree_directories(&staging)?;
        rename_noreplace(&staging, destination)?;
        sync_directory(parent)?;
        Ok(record)
    }

    fn build_portable_export(
        &self,
        staging: &Path,
        destination: &Path,
    ) -> Result<PortableExportRecord> {
        let before = self.summary()?;
        let blocks = self.list_blocks()?;
        let graph = self.graph()?;
        let variables = self.variables()?;
        let exported_at = utc_now();
        fs::create_dir(staging.join("blocks"))?;
        fs::create_dir(staging.join("attachments"))?;

        let mut files = Vec::new();
        let mut citations = BTreeMap::<String, CitationExport>::new();
        let mut attachment_count = 0usize;

        for block in &blocks {
            let attachments = self.attachments_for_block(&block.snapshot.id)?;
            let block_citations = self.citations_for_block(&block.snapshot.id)?;
            let attachment_links = self.export_attachments(
                staging,
                block,
                &attachments,
                &mut files,
            )?;
            attachment_count += attachments.len();
            for link in &block_citations {
                let entry = citations
                    .entry(link.citation.id.clone())
                    .or_insert_with(|| CitationExport {
                        citation: link.citation.clone(),
                        block_ids: Vec::new(),
                    });
                if !entry.block_ids.contains(&block.snapshot.id) {
                    entry.block_ids.push(block.snapshot.id.clone());
                }
            }
            let markdown = block_markdown(block, &attachment_links, &block_citations);
            let relative = PathBuf::from("blocks").join(format!(
                "{}-{}.md",
                block.snapshot.id,
                safe_component(&block.snapshot.title)
            ));
            write_export_file(staging, &relative, markdown.as_bytes(), &mut files)?;
        }

        let citation_values = citations.into_values().collect::<Vec<_>>();
        write_export_file(
            staging,
            Path::new("citations.json"),
            serde_json::to_string_pretty(&citation_values)?.as_bytes(),
            &mut files,
        )?;
        write_export_file(
            staging,
            Path::new("citations.bib"),
            citations_bib(&citation_values).as_bytes(),
            &mut files,
        )?;
        write_export_file(
            staging,
            Path::new("relations.json"),
            serde_json::to_string_pretty(&graph.edges)?.as_bytes(),
            &mut files,
        )?;
        write_export_file(
            staging,
            Path::new("variables.json"),
            serde_json::to_string_pretty(&variables)?.as_bytes(),
            &mut files,
        )?;

        let after = self.summary()?;
        if after.revision != before.revision {
            return Err(SanctumError::Conflict {
                block_id: "portable-export".into(),
                expected: before.revision,
                actual: after.revision,
            });
        }

        files.sort_by(|left, right| left.path.cmp(&right.path));
        let manifest = ExportManifest {
            format_version: 1,
            vault_id: before.vault_id,
            vault_name: before.name,
            source_revision: before.revision,
            exported_at: exported_at.clone(),
            block_count: blocks.len(),
            attachment_count,
            citation_count: citation_values.len(),
            files,
        };
        let manifest_path = staging.join("manifest.json");
        atomic_write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest)?.as_bytes(),
        )?;
        let manifest_sha256 = sha256_file(&manifest_path)?;

        Ok(PortableExportRecord {
            destination_path: destination.to_string_lossy().into_owned(),
            exported_at,
            source_revision: before.revision,
            block_count: blocks.len(),
            attachment_count,
            citation_count: citation_values.len(),
            manifest_sha256,
        })
    }

    fn export_attachments(
        &self,
        staging: &Path,
        block: &HypothesisBlock,
        attachments: &[Attachment],
        files: &mut Vec<ExportFile>,
    ) -> Result<Vec<(Attachment, String)>> {
        let mut links = Vec::new();
        for attachment in attachments {
            let source = self.attachment_object_path(&attachment.id)?;
            let relative = PathBuf::from("attachments")
                .join(&block.snapshot.id)
                .join(format!(
                    "{}-{}",
                    attachment.id,
                    safe_component(&attachment.display_name)
                ));
            let target = staging.join(&relative);
            fs::create_dir_all(target.parent().expect("attachment export parent"))?;
            fs::copy(&source, &target)?;
            sync_file(&target)?;
            let exported_hash = sha256_file(&target)?;
            if exported_hash != attachment.object_hash {
                return Err(SanctumError::Integrity(format!(
                    "exported attachment {} failed SHA-256 verification",
                    attachment.id
                )));
            }
            files.push(export_file(staging, &relative)?);
            links.push((
                attachment.clone(),
                format!("../{}", path_for_markdown(&relative)),
            ));
        }
        Ok(links)
    }
}

fn block_markdown(
    block: &HypothesisBlock,
    attachments: &[(Attachment, String)],
    citations: &[BlockCitationRecord],
) -> String {
    let mut output = format!(
        "# {}\n\n- ID: `{}`\n- Type: `{}`\n- Status: `{}`\n- Updated: `{}`\n",
        block.snapshot.title,
        block.snapshot.id,
        block.snapshot.kind.as_str(),
        block.snapshot.status.as_str(),
        block.updated_at
    );
    if !block.snapshot.tags.is_empty() {
        output.push_str(&format!("- Tags: {}\n", block.snapshot.tags.join(", ")));
    }
    output.push_str("\n## 本文\n\n");
    output.push_str(&block.snapshot.body_markdown);
    output.push_str("\n\n## 研究ノート\n\n");
    output.push_str(&block.snapshot.research_notes_markdown);
    if !attachments.is_empty() {
        output.push_str("\n\n## 添付ファイル\n\n");
        for (attachment, path) in attachments {
            output.push_str(&format!(
                "- [{}](<{}>) — {}\n",
                attachment.display_name.replace('[', "\\[").replace(']', "\\]"),
                path,
                attachment.relation_type.as_str()
            ));
        }
    }
    if !citations.is_empty() {
        output.push_str("\n\n## 文献\n\n");
        for link in citations {
            output.push_str(&format!(
                "- [@{}] {}\n",
                link.citation.citation_key, link.citation.title
            ));
        }
    }
    output.push('\n');
    output
}

fn citations_bib(citations: &[CitationExport]) -> String {
    let mut output = String::new();
    for item in citations {
        let citation = &item.citation;
        let kind = if citation.doi.is_some() { "article" } else { "misc" };
        output.push_str(&format!(
            "@{kind}{{{},\n  title = {{{}}}",
            safe_bib_key(&citation.citation_key),
            escape_bib(&citation.title)
        ));
        if !citation.authors.trim().is_empty() {
            output.push_str(&format!(",\n  author = {{{}}}", escape_bib(&citation.authors)));
        }
        if let Some(year) = citation.year {
            output.push_str(&format!(",\n  year = {{{year}}}"));
        }
        if let Some(doi) = citation.doi.as_deref() {
            output.push_str(&format!(",\n  doi = {{{}}}", escape_bib(doi)));
        }
        if let Some(url) = citation.url.as_deref() {
            output.push_str(&format!(",\n  url = {{{}}}", escape_bib(url)));
        }
        output.push_str("\n}\n\n");
    }
    output
}

fn write_export_file(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    files: &mut Vec<ExportFile>,
) -> Result<()> {
    atomic_write(&root.join(relative), bytes)?;
    files.push(export_file(root, relative)?);
    Ok(())
}

fn export_file(root: &Path, relative: &Path) -> Result<ExportFile> {
    let path = root.join(relative);
    Ok(ExportFile {
        path: path_for_markdown(relative),
        sha256: sha256_file(&path)?,
        byte_size: path.metadata()?.len(),
    })
}

fn path_for_markdown(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn unique_destination(parent: &Path, stem: &str) -> PathBuf {
    let proposed = parent.join(stem);
    if !proposed.exists() {
        return proposed;
    }
    for suffix in 2..10_000 {
        let candidate = parent.join(format!("{stem}-{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-{}", Uuid::new_v4()))
}

fn safe_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect::<String>();
    let cleaned = cleaned.trim_matches(|character: char| character == ' ' || character == '.');
    let cleaned = if cleaned.is_empty() { "untitled" } else { cleaned };
    cleaned
        .chars()
        .take(100)
        .collect::<String>()
        .trim_matches(|character: char| character == ' ' || character == '.')
        .to_owned()
}

fn safe_bib_key(value: &str) -> String {
    let value = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':'))
        .collect::<String>();
    if value.is_empty() { "citation".into() } else { value }
}

fn escape_bib(value: &str) -> String {
    value.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_export_names_cannot_escape_their_directory() {
        assert_eq!(safe_component("../a:b?.pdf"), "-a-b-.pdf");
        assert_eq!(safe_bib_key("Doe 2026 / test"), "Doe2026test");
    }
}
