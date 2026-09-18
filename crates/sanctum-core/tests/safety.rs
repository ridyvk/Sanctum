use rusqlite::Connection;
use sanctum_core::*;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn new_vault() -> (TempDir, PathBuf, Vault) {
    let temporary = tempfile::tempdir().expect("tempdir");
    let path = temporary.path().join("Research.sanctum");
    let vault = Vault::create(&path, "Research").expect("create vault");
    (temporary, path, vault)
}

fn create_input(title: &str) -> CreateBlockInput {
    CreateBlockInput {
        title: title.into(),
        body_markdown: format!("# {title}\n\nInitial $x = 1$"),
        research_notes_markdown: "A durable note".into(),
        kind: BlockKind::Hypothesis,
        status: BlockStatus::Idea,
        tags: vec!["economics".into()],
        change_reason: "Initial hypothesis".into(),
    }
}

fn save_input(block: &HypothesisBlock, body: &str, reason: &str) -> SaveBlockInput {
    SaveBlockInput {
        block_id: block.snapshot.id.clone(),
        expected_row_version: block.row_version,
        title: block.snapshot.title.clone(),
        body_markdown: body.into(),
        research_notes_markdown: block.snapshot.research_notes_markdown.clone(),
        kind: block.snapshot.kind,
        status: BlockStatus::Developing,
        tags: block.snapshot.tags.clone(),
        change_reason: reason.into(),
    }
}

#[test]
fn vault_uses_wal_and_committed_blocks_survive_reopen() {
    let (_temporary, path, vault) = new_vault();
    let block = vault
        .create_block(create_input("Mortality convergence"))
        .unwrap();
    let database = Connection::open(path.join("db/sanctum.sqlite")).unwrap();
    let journal_mode: String = database
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let synchronous: i64 = database
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");
    assert_eq!(synchronous, 2, "FULL synchronous is required");
    drop(database);
    drop(vault);
    let reopened = Vault::open(path).unwrap();
    assert_eq!(
        reopened
            .get_block(&block.snapshot.id)
            .unwrap()
            .snapshot
            .title,
        "Mortality convergence"
    );
}

#[test]
fn stale_save_is_rejected_without_losing_the_committed_edit() {
    let (_temporary, _path, vault) = new_vault();
    let original = vault.create_block(create_input("Competition")).unwrap();
    let committed = vault
        .save_block(save_input(&original, "first writer", "First edit"))
        .unwrap();
    let error = vault
        .save_block(save_input(&original, "stale writer", "Stale edit"))
        .unwrap_err();
    assert!(matches!(error, SanctumError::Conflict { .. }));
    assert_eq!(
        vault
            .get_block(&original.snapshot.id)
            .unwrap()
            .snapshot
            .body_markdown,
        "first writer"
    );
    assert_eq!(committed.row_version, original.row_version + 1);
    assert_eq!(vault.versions(&original.snapshot.id).unwrap().len(), 2);
}

#[test]
fn unchanged_save_does_not_append_a_version_or_increment_revision() {
    let (_temporary, _path, vault) = new_vault();
    let original = vault.create_block(create_input("Stable content")).unwrap();
    let saved = vault
        .save_block(SaveBlockInput {
            block_id: original.snapshot.id.clone(),
            expected_row_version: original.row_version,
            title: original.snapshot.title.clone(),
            body_markdown: original.snapshot.body_markdown.clone(),
            research_notes_markdown: original.snapshot.research_notes_markdown.clone(),
            kind: original.snapshot.kind,
            status: original.snapshot.status,
            tags: original.snapshot.tags.clone(),
            change_reason: "Redundant autosave".into(),
        })
        .unwrap();

    assert_eq!(saved.row_version, original.row_version);
    assert_eq!(saved.current_version_id, original.current_version_id);
    assert_eq!(vault.versions(&original.snapshot.id).unwrap().len(), 1);
}

#[test]
fn restoring_a_block_appends_history_instead_of_rewriting_it() {
    let (_temporary, _path, vault) = new_vault();
    let initial = vault.create_block(create_input("Inheritance")).unwrap();
    let first_version = vault.versions(&initial.snapshot.id).unwrap().pop().unwrap();
    let changed = vault
        .save_block(save_input(&initial, "changed model", "Revised model"))
        .unwrap();
    let restored = vault
        .restore_block_version(
            &initial.snapshot.id,
            &first_version.id,
            "Return to the original assumptions",
        )
        .unwrap();
    let versions = vault.versions(&initial.snapshot.id).unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(
        restored.snapshot.body_markdown,
        initial.snapshot.body_markdown
    );
    assert_eq!(restored.row_version, changed.row_version + 1);
    assert_eq!(
        versions.last().unwrap().content_sha256,
        first_version.content_sha256
    );
}

#[test]
fn semantic_graph_and_branch_lineage_persist() {
    let (_temporary, path, vault) = new_vault();
    let parent = vault
        .create_block(create_input("Market structure"))
        .unwrap();
    let support = vault.create_block(create_input("Observed prices")).unwrap();
    let child = vault
        .branch_block(BranchBlockInput {
            parent_block_id: parent.snapshot.id.clone(),
            expected_parent_row_version: parent.row_version,
            title: "Market structure — monopoly".into(),
            branch_reason: "Explore monopoly assumptions".into(),
        })
        .unwrap();
    vault
        .create_edge(CreateEdgeInput {
            source_block_id: support.snapshot.id.clone(),
            target_block_id: child.snapshot.id.clone(),
            edge_type: EdgeType::Supports,
            note: "Observed concentration".into(),
        })
        .unwrap();
    vault
        .set_graph_position(GraphPosition {
            block_id: child.snapshot.id.clone(),
            view_id: "main".into(),
            x: 120.0,
            y: -30.5,
        })
        .unwrap();
    drop(vault);
    let reopened = Vault::open(path).unwrap();
    let graph = reopened.graph().unwrap();
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].edge_type, EdgeType::Supports);
    assert_eq!(graph.positions[0].x, 120.0);
    assert_eq!(
        reopened
            .get_block(&child.snapshot.id)
            .unwrap()
            .parent_block_id,
        Some(parent.snapshot.id)
    );
}

#[test]
fn variable_conflicts_citations_and_full_text_search_are_structured() {
    let (_temporary, _path, vault) = new_vault();
    let first = vault.create_block(create_input("Mortality")).unwrap();
    let second = vault.create_block(create_input("Migration")).unwrap();
    vault
        .register_variable_definition(VariableDefinitionInput {
            symbol: "μ".into(),
            definition: "mortality rate".into(),
            block_id: first.snapshot.id.clone(),
            formula: "μ(a,t)".into(),
        })
        .unwrap();
    let variable = vault
        .register_variable_definition(VariableDefinitionInput {
            symbol: "μ".into(),
            definition: "migration intensity".into(),
            block_id: second.snapshot.id.clone(),
            formula: "μ_t".into(),
        })
        .unwrap();
    assert!(variable.has_conflict);
    let citation_input = CitationInput {
        citation_key: "smith2024".into(),
        title: "Mortality and Wealth".into(),
        authors: "Smith".into(),
        year: Some(2024),
        doi: Some("10.1000/example".into()),
        url: None,
        raw_csl_json: serde_json::json!({}),
    };
    vault
        .add_citation(
            &first.snapshot.id,
            citation_input.clone(),
            "Mortality falls with access",
            serde_json::json!({"page": 17}),
        )
        .unwrap();
    vault
        .add_citation(
            &first.snapshot.id,
            citation_input,
            "Updated quote",
            serde_json::json!({"page": 18}),
        )
        .unwrap();
    let citations = vault.citations_for_block(&first.snapshot.id).unwrap();
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].quote_text, "Updated quote");
    let hits = vault.search("mortality", 20).unwrap();
    assert!(hits.iter().any(|hit| hit.block_id == first.snapshot.id));
    let report = vault.integrity_check().unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "variable_definition_conflict"));
}

#[test]
fn soft_delete_never_removes_history_and_dangling_relations_are_reported() {
    let (_temporary, _path, vault) = new_vault();
    let dependent = vault.create_block(create_input("Dependent")).unwrap();
    let premise = vault.create_block(create_input("Premise")).unwrap();
    vault
        .create_edge(CreateEdgeInput {
            source_block_id: dependent.snapshot.id.clone(),
            target_block_id: premise.snapshot.id.clone(),
            edge_type: EdgeType::DependsOn,
            note: String::new(),
        })
        .unwrap();
    vault
        .soft_delete_block(&premise.snapshot.id, premise.row_version)
        .unwrap();
    assert!(matches!(
        vault.get_block(&premise.snapshot.id),
        Err(SanctumError::BlockNotFound(_))
    ));
    assert_eq!(vault.versions(&premise.snapshot.id).unwrap().len(), 1);
    assert_eq!(vault.list_deleted_blocks().unwrap().len(), 1);
    assert!(vault
        .integrity_check()
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.code == "dangling_edge"));
    let deleted = vault.list_deleted_blocks().unwrap().pop().unwrap();
    vault
        .restore_deleted_block(&deleted.snapshot.id, deleted.row_version)
        .unwrap();
    assert_eq!(
        vault
            .get_block(&premise.snapshot.id)
            .unwrap()
            .snapshot
            .title,
        "Premise"
    );
}

#[test]
fn attachments_deduplicate_and_bit_flips_are_detected() {
    let (temporary, _path, vault) = new_vault();
    let block = vault
        .create_block(create_input("Dataset evidence"))
        .unwrap();
    let source = temporary.path().join("data.csv");
    fs::write(&source, b"age,mortality\n40,0.01\n").unwrap();
    let first = vault
        .attach_file(
            &block.snapshot.id,
            &source,
            AttachmentRelation::Dataset,
            serde_json::json!({}),
        )
        .unwrap();
    let second = vault
        .attach_file(
            &block.snapshot.id,
            &source,
            AttachmentRelation::Supports,
            serde_json::json!({"row": 2}),
        )
        .unwrap();
    assert_eq!(first.object_hash, second.object_hash);
    assert!(vault
        .search("data", 20)
        .unwrap()
        .iter()
        .any(|hit| hit.block_id == block.snapshot.id));
    let object = vault.attachment_object_path(&first.id).unwrap();
    let mut bytes = fs::read(&object).unwrap();
    bytes[0] ^= 0xff;
    fs::write(&object, bytes).unwrap();
    let report = vault.integrity_check().unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "object_hash_mismatch"));
}

#[test]
fn snapshot_restores_a_verified_clone_without_overwriting_current_state() {
    let (temporary, _path, vault) = new_vault();
    let block = vault.create_block(create_input("Snapshot state")).unwrap();
    let snapshot = vault.create_snapshot(SnapshotKind::Manual).unwrap();
    let changed = vault
        .save_block(save_input(&block, "newer state", "After snapshot"))
        .unwrap();
    let destination = temporary.path().join("Restored.sanctum");
    vault
        .restore_snapshot_to(&snapshot.id, &destination)
        .unwrap();
    let restored = Vault::open(&destination).unwrap();
    assert_eq!(
        restored
            .get_block(&block.snapshot.id)
            .unwrap()
            .snapshot
            .body_markdown,
        block.snapshot.body_markdown
    );
    assert_eq!(
        vault
            .get_block(&block.snapshot.id)
            .unwrap()
            .snapshot
            .body_markdown,
        changed.snapshot.body_markdown
    );
    assert!(matches!(
        vault.restore_snapshot_to(&snapshot.id, &destination),
        Err(SanctumError::RefuseOverwrite(_))
    ));
}

#[test]
fn automatic_snapshot_policy_does_not_duplicate_an_unchanged_revision() {
    let (_temporary, _path, vault) = new_vault();
    vault
        .create_block(create_input("Automatic snapshot"))
        .unwrap();
    let first = vault.create_due_snapshots().unwrap();
    assert_eq!(first.len(), 3);
    let second = vault.create_due_snapshots().unwrap();
    assert!(second.is_empty());
}

#[test]
fn encrypted_backup_round_trip_and_authentication_fail_closed() {
    let (temporary, _path, vault) = new_vault();
    let block = vault
        .create_block(create_input("Encrypted archive"))
        .unwrap();
    let source = temporary.path().join("paper.pdf");
    fs::write(&source, b"%PDF-1.7\nresearch\n").unwrap();
    vault
        .attach_file(
            &block.snapshot.id,
            &source,
            AttachmentRelation::Reference,
            serde_json::json!({"page": 2}),
        )
        .unwrap();
    let archive = temporary.path().join("external.sanctum-backup");
    let record = vault
        .create_encrypted_backup(&archive, "correct horse battery staple")
        .unwrap();
    assert!(record.byte_size > 0);
    Vault::verify_encrypted_backup(&archive, "correct horse battery staple").unwrap();
    assert!(matches!(
        Vault::verify_encrypted_backup(&archive, "wrong password indeed"),
        Err(SanctumError::Authentication)
    ));
    let restored_path = temporary.path().join("BackupRestore.sanctum");
    Vault::restore_encrypted_backup_to(&archive, "correct horse battery staple", &restored_path)
        .unwrap();
    let restored = Vault::open(restored_path).unwrap();
    assert_eq!(
        restored
            .get_block(&block.snapshot.id)
            .unwrap()
            .snapshot
            .title,
        "Encrypted archive"
    );
    assert_eq!(
        restored
            .attachments_for_block(&block.snapshot.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_missing_database_is_not_silently_recreated() {
    let (_temporary, path, vault) = new_vault();
    drop(vault);
    fs::rename(
        path.join("db/sanctum.sqlite"),
        path.join("quarantine/lost.sqlite"),
    )
    .unwrap();
    assert!(matches!(
        Vault::open(&path),
        Err(SanctumError::VaultMissing(_))
    ));
    assert!(!path.join("db/sanctum.sqlite").exists());
}

#[test]
fn crash_draft_survives_reopen_and_only_matching_commit_clears_it() {
    let (_temporary, path, vault) = new_vault();
    let block = vault.create_block(create_input("Crash draft")).unwrap();
    let draft_input = save_input(&block, "unsaved but durable draft", "Recovered after crash");
    let draft = vault.persist_recovery_draft(draft_input.clone()).unwrap();
    drop(vault);
    let reopened = Vault::open(&path).unwrap();
    let recovered = reopened
        .recovery_draft(&block.snapshot.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.snapshot.body_markdown,
        "unsaved but durable draft"
    );
    assert_eq!(recovered.content_sha256, draft.content_sha256);
    reopened.save_block(draft_input).unwrap();
    assert!(reopened
        .recovery_draft(&block.snapshot.id)
        .unwrap()
        .is_none());
}

#[test]
fn immutable_version_hash_mismatch_is_reported_as_fatal() {
    let (_temporary, path, vault) = new_vault();
    let block = vault
        .create_block(create_input("Immutable history"))
        .unwrap();
    drop(vault);
    let database_path = path.join("db/sanctum.sqlite");
    let connection = Connection::open(&database_path).unwrap();
    assert!(
        connection
            .execute(
                "UPDATE block_versions SET content_sha256=?1 WHERE block_id=?2",
                rusqlite::params!["0".repeat(64), block.snapshot.id]
            )
            .is_err(),
        "immutability trigger must reject ordinary tampering"
    );
    connection
        .execute_batch("DROP TRIGGER block_versions_no_update;")
        .unwrap();
    connection
        .execute(
            "UPDATE block_versions SET content_sha256=?1 WHERE block_id=?2",
            rusqlite::params!["0".repeat(64), block.snapshot.id],
        )
        .unwrap();
    drop(connection);
    assert!(
        matches!(Vault::open(path), Err(SanctumError::Integrity(message)) if message.contains("failed SHA-256"))
    );
}

#[test]
fn corrupt_compressed_snapshot_is_rejected_without_publishing_destination() {
    let (temporary, _path, vault) = new_vault();
    vault
        .create_block(create_input("Corrupt snapshot"))
        .unwrap();
    let snapshot = vault.create_snapshot(SnapshotKind::Manual).unwrap();
    let compressed = Path::new(&snapshot.path).join("sanctum.sqlite.zst");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&compressed)
        .unwrap();
    let length = file.metadata().unwrap().len();
    file.seek(SeekFrom::Start(length / 2)).unwrap();
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 0x5a;
    file.seek(SeekFrom::Start(length / 2)).unwrap();
    file.write_all(&byte).unwrap();
    file.sync_all().unwrap();
    let destination = temporary.path().join("MustNotExist.sanctum");
    assert!(vault
        .restore_snapshot_to(&snapshot.id, &destination)
        .is_err());
    assert!(!destination.exists());
}

#[test]
fn pending_journal_outbox_is_drained_on_open() {
    let (_temporary, path, vault) = new_vault();
    vault
        .create_block(create_input("Journal recovery"))
        .unwrap();
    drop(vault);
    fs::remove_file(path.join("journal/00000000000000000001.json")).unwrap();
    let connection = Connection::open(path.join("db/sanctum.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE journal_outbox SET dispatched_at=NULL WHERE revision=1",
            [],
        )
        .unwrap();
    drop(connection);
    let reopened = Vault::open(&path).unwrap();
    assert!(path.join("journal/00000000000000000001.json").is_file());
    assert_eq!(reopened.summary().unwrap().revision, 1);
}

#[test]
fn migration_checksum_tampering_fails_closed() {
    let (_temporary, path, vault) = new_vault();
    drop(vault);
    let connection = Connection::open(path.join("db/sanctum.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE schema_migrations SET checksum=?1 WHERE version=1",
            ["0".repeat(64)],
        )
        .unwrap();
    drop(connection);
    assert!(
        matches!(Vault::open(path), Err(SanctumError::Integrity(message)) if message.contains("migration 1 checksum"))
    );
}

#[test]
fn an_unrecognized_database_is_rejected_without_migration_writes() {
    let (_temporary, path, vault) = new_vault();
    drop(vault);
    let database_path = path.join("db/sanctum.sqlite");
    fs::remove_file(&database_path).unwrap();
    let foreign = Connection::open(&database_path).unwrap();
    foreign
        .execute("CREATE TABLE unrelated(value TEXT)", [])
        .unwrap();
    drop(foreign);
    let before = fs::read(&database_path).unwrap();
    assert!(
        matches!(Vault::open(&path), Err(SanctumError::Integrity(message)) if message.contains("not a recognized Sanctum"))
    );
    assert_eq!(fs::read(&database_path).unwrap(), before);
}

#[test]
fn integrity_reports_research_changes_newer_than_the_latest_backup() {
    let (temporary, _path, vault) = new_vault();
    let block = vault.create_block(create_input("Backup age")).unwrap();
    let archive = temporary.path().join("age.sanctum-backup");
    vault
        .create_encrypted_backup(&archive, "long and separate backup password")
        .unwrap();
    assert!(!vault
        .integrity_check()
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.code == "unbacked_changes"));
    vault
        .save_block(save_input(
            &block,
            "new research after backup",
            "New result",
        ))
        .unwrap();
    assert!(vault
        .integrity_check()
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.code == "unbacked_changes"));
}

#[test]
fn encrypted_backup_bit_flip_fails_authentication() {
    let (temporary, _path, vault) = new_vault();
    vault
        .create_block(create_input("Authenticated backup"))
        .unwrap();
    let archive = temporary.path().join("tamper.sanctum-backup");
    vault
        .create_encrypted_backup(&archive, "a sufficiently long backup password")
        .unwrap();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&archive)
        .unwrap();
    let last = file.metadata().unwrap().len() - 1;
    file.seek(SeekFrom::Start(last)).unwrap();
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(last)).unwrap();
    file.write_all(&byte).unwrap();
    file.sync_all().unwrap();
    assert!(matches!(
        Vault::verify_encrypted_backup(&archive, "a sufficiently long backup password"),
        Err(SanctumError::Authentication)
    ));
}

#[test]
fn a_verified_snapshot_published_before_its_record_is_reconciled_on_open() {
    let (_temporary, path, vault) = new_vault();
    vault
        .create_block(create_input("Snapshot reconciliation"))
        .unwrap();
    let snapshot = vault.create_snapshot(SnapshotKind::Manual).unwrap();
    let source = PathBuf::from(&snapshot.path);
    let orphan = path.join("snapshots/recoverable-orphan");
    fs::create_dir(&orphan).unwrap();
    fs::copy(
        source.join("sanctum.sqlite.zst"),
        orphan.join("sanctum.sqlite.zst"),
    )
    .unwrap();
    let mut manifest: SnapshotManifest =
        serde_json::from_slice(&fs::read(source.join("manifest.json")).unwrap()).unwrap();
    manifest.snapshot_id = "published-before-record".into();
    fs::write(
        orphan.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    drop(vault);

    let reopened = Vault::open(&path).unwrap();
    let snapshots = reopened.snapshots().unwrap();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots
        .iter()
        .any(|item| item.id == "published-before-record"));
}

#[test]
fn portable_export_contains_human_readable_data_and_verified_attachments() {
    let (temporary, _path, vault) = new_vault();
    let first = vault.create_block(create_input("Portable model")).unwrap();
    let second = vault.create_block(create_input("Evidence base")).unwrap();
    vault
        .create_edge(CreateEdgeInput {
            source_block_id: second.snapshot.id.clone(),
            target_block_id: first.snapshot.id.clone(),
            edge_type: EdgeType::Supports,
            note: "Replication evidence".into(),
        })
        .unwrap();
    vault
        .register_variable_definition(VariableDefinitionInput {
            symbol: "x".into(),
            definition: "treatment".into(),
            block_id: first.snapshot.id.clone(),
            formula: "x_i".into(),
        })
        .unwrap();
    vault
        .add_citation(
            &first.snapshot.id,
            CitationInput {
                citation_key: "smith2026".into(),
                title: "Durable Research".into(),
                authors: "Jane Smith".into(),
                year: Some(2026),
                doi: Some("10.1000/durable".into()),
                url: None,
                raw_csl_json: serde_json::json!({"type": "article-journal"}),
            },
            "",
            serde_json::json!({}),
        )
        .unwrap();
    let attachment_source = temporary.path().join("evidence.csv");
    fs::write(&attachment_source, b"year,value\n2026,1\n").unwrap();
    let attachment = vault
        .attach_file(
            &first.snapshot.id,
            &attachment_source,
            AttachmentRelation::Dataset,
            serde_json::json!({}),
        )
        .unwrap();

    let export_parent = temporary.path().join("exports");
    fs::create_dir(&export_parent).unwrap();
    let revision_before = vault.summary().unwrap().revision;
    let exported = vault.export_portable_to(&export_parent).unwrap();
    assert_eq!(exported.source_revision, revision_before);
    assert_eq!(exported.block_count, 2);
    assert_eq!(exported.attachment_count, 1);
    assert_eq!(exported.citation_count, 1);
    assert_eq!(vault.summary().unwrap().revision, revision_before);

    let destination = PathBuf::from(&exported.destination_path);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap();
    for entry in files {
        let relative = entry["path"].as_str().unwrap();
        let bytes = fs::read(destination.join(relative)).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&bytes)), entry["sha256"]);
    }
    assert!(fs::read_to_string(destination.join("citations.bib"))
        .unwrap()
        .contains("@article{smith2026"));
    let exported_attachment = files
        .iter()
        .find(|entry| entry["path"].as_str().unwrap().contains("attachments/"))
        .unwrap();
    assert_eq!(exported_attachment["sha256"], attachment.object_hash);

    assert!(matches!(
        vault.export_portable(&destination),
        Err(SanctumError::RefuseOverwrite(_))
    ));
}

#[test]
fn automatic_backup_configuration_is_append_only_and_due_aware() {
    let (temporary, path, vault) = new_vault();
    let destination = temporary.path().join("external-backups");
    fs::create_dir(&destination).unwrap();

    let configured = vault
        .save_automatic_backup_config(true, &destination, None)
        .unwrap();
    assert!(vault.automatic_backup_due(&configured).unwrap());
    let proposed = vault.next_automatic_backup_path(&configured).unwrap();
    assert_eq!(proposed.parent(), Some(destination.as_path()));
    assert_eq!(
        proposed.extension().and_then(|value| value.to_str()),
        Some("sanctum-backup")
    );

    let completed = vault
        .save_automatic_backup_config(
            true,
            &destination,
            Some(chrono::Utc::now().to_rfc3339()),
        )
        .unwrap();
    assert!(!vault.automatic_backup_due(&completed).unwrap());
    drop(vault);

    let reopened = Vault::open(path).unwrap();
    assert_eq!(reopened.automatic_backup_config().unwrap(), completed);
    let records = fs::read_dir(reopened.root().join("settings/automatic-backup"))
        .unwrap()
        .count();
    assert_eq!(records, 2);
}
