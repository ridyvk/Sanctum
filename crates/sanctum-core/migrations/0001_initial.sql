PRAGMA application_id = 1396788803;

CREATE TABLE vault_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    vault_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    created_at TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
) STRICT;

CREATE TABLE blocks (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('Hypothesis','Assumption','Method','Evidence')),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    body_markdown TEXT NOT NULL DEFAULT '',
    research_notes_markdown TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('Idea','Developing','Testing','Supported','Weakly Supported','Rejected','Archived')),
    current_version_id TEXT,
    parent_block_id TEXT REFERENCES blocks(id) ON DELETE RESTRICT,
    row_version INTEGER NOT NULL CHECK (row_version > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE TABLE block_versions (
    id TEXT PRIMARY KEY,
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    version_index INTEGER NOT NULL CHECK (version_index > 0),
    version_label TEXT NOT NULL,
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    change_reason TEXT NOT NULL CHECK (length(trim(change_reason)) > 0),
    created_at TEXT NOT NULL,
    UNIQUE(block_id, version_index),
    UNIQUE(block_id, version_label)
) STRICT;

CREATE UNIQUE INDEX blocks_current_version_unique ON blocks(current_version_id) WHERE current_version_id IS NOT NULL;

CREATE TRIGGER blocks_current_version_belongs_to_block
BEFORE UPDATE OF current_version_id ON blocks
WHEN NEW.current_version_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM block_versions v WHERE v.id = NEW.current_version_id AND v.block_id = NEW.id
    ) THEN RAISE(ABORT, 'current version must belong to block') END;
END;

CREATE TABLE block_drafts (
    block_id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE RESTRICT,
    base_row_version INTEGER NOT NULL CHECK (base_row_version > 0),
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE block_lineage (
    child_block_id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE RESTRICT,
    parent_block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    parent_version_id TEXT NOT NULL REFERENCES block_versions(id) ON DELETE RESTRICT,
    branch_reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CHECK (child_block_id <> parent_block_id)
) STRICT;

CREATE TABLE edges (
    id TEXT PRIMARY KEY,
    source_block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    target_block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    edge_type TEXT NOT NULL CHECK (edge_type IN ('Supports','Contradicts','Depends on','Derived from','Assumes','Extends','Tests','Alternative to','Related to')),
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK (source_block_id <> target_block_id)
) STRICT;
CREATE INDEX edges_source_active ON edges(source_block_id) WHERE deleted_at IS NULL;
CREATE INDEX edges_target_active ON edges(target_block_id) WHERE deleted_at IS NULL;

CREATE TABLE graph_positions (
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    view_id TEXT NOT NULL DEFAULT 'main',
    x REAL NOT NULL,
    y REAL NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(block_id, view_id)
) STRICT;

CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK (length(trim(name)) > 0),
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE block_tags (
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE RESTRICT,
    PRIMARY KEY(block_id, tag_id)
) STRICT;

CREATE TABLE variables (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK (length(trim(symbol)) > 0),
    created_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE TABLE variable_definitions (
    id TEXT PRIMARY KEY,
    variable_id TEXT NOT NULL REFERENCES variables(id) ON DELETE RESTRICT,
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    definition TEXT NOT NULL CHECK (length(trim(definition)) > 0),
    normalized_definition TEXT NOT NULL,
    formula TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE TABLE variable_occurrences (
    id TEXT PRIMARY KEY,
    variable_id TEXT NOT NULL REFERENCES variables(id) ON DELETE RESTRICT,
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    formula TEXT NOT NULL,
    start_offset INTEGER,
    end_offset INTEGER,
    created_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE TABLE objects (
    sha256 TEXT PRIMARY KEY CHECK (length(sha256) = 64),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    media_type TEXT,
    original_extension TEXT,
    created_at TEXT NOT NULL,
    last_verified_at TEXT NOT NULL
) STRICT;

CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    object_hash TEXT NOT NULL REFERENCES objects(sha256) ON DELETE RESTRICT,
    relation_type TEXT NOT NULL CHECK (relation_type IN ('Supports','Contradicts','Background','Method','Dataset','Reference','Other')),
    display_name TEXT NOT NULL,
    locator_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(locator_json)),
    created_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;
CREATE INDEX attachments_block_active ON attachments(block_id) WHERE deleted_at IS NULL;

CREATE TABLE citations (
    id TEXT PRIMARY KEY,
    citation_key TEXT NOT NULL COLLATE NOCASE UNIQUE,
    title TEXT NOT NULL,
    authors TEXT NOT NULL DEFAULT '',
    year INTEGER,
    doi TEXT,
    url TEXT,
    raw_csl_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(raw_csl_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE TABLE block_citations (
    id TEXT PRIMARY KEY,
    block_id TEXT NOT NULL REFERENCES blocks(id) ON DELETE RESTRICT,
    citation_id TEXT NOT NULL REFERENCES citations(id) ON DELETE RESTRICT,
    quote_text TEXT NOT NULL DEFAULT '',
    locator_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(locator_json)),
    created_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE VIRTUAL TABLE block_fts USING fts5(
    block_id UNINDEXED,
    title,
    body_markdown,
    research_notes_markdown,
    tags,
    variables,
    citation_metadata,
    file_metadata,
    tokenize = 'unicode61'
);

CREATE TABLE change_events (
    revision INTEGER PRIMARY KEY CHECK (revision > 0),
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    previous_event_hash TEXT,
    event_hash TEXT NOT NULL UNIQUE CHECK (length(event_hash) = 64),
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE journal_outbox (
    revision INTEGER PRIMARY KEY REFERENCES change_events(revision) ON DELETE RESTRICT,
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),
    dispatched_at TEXT
) STRICT;

CREATE TABLE snapshots (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('TenMinute','Daily','Weekly','Manual')),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    relative_path TEXT NOT NULL UNIQUE,
    database_sha256 TEXT NOT NULL CHECK (length(database_sha256) = 64),
    created_at TEXT NOT NULL,
    verified_at TEXT NOT NULL
) STRICT;

CREATE TABLE backups (
    id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    file_name TEXT NOT NULL,
    destination_path TEXT NOT NULL UNIQUE,
    archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    created_at TEXT NOT NULL,
    verified_at TEXT NOT NULL
) STRICT;

CREATE TRIGGER blocks_no_physical_delete BEFORE DELETE ON blocks BEGIN SELECT RAISE(ABORT, 'blocks are soft-delete only'); END;
CREATE TRIGGER block_versions_no_update BEFORE UPDATE ON block_versions BEGIN SELECT RAISE(ABORT, 'block versions are immutable'); END;
CREATE TRIGGER block_versions_no_delete BEFORE DELETE ON block_versions BEGIN SELECT RAISE(ABORT, 'block versions are immutable'); END;
CREATE TRIGGER block_lineage_no_update BEFORE UPDATE ON block_lineage BEGIN SELECT RAISE(ABORT, 'block lineage is immutable'); END;
CREATE TRIGGER block_lineage_no_delete BEFORE DELETE ON block_lineage BEGIN SELECT RAISE(ABORT, 'block lineage is immutable'); END;
CREATE TRIGGER change_events_no_update BEFORE UPDATE ON change_events BEGIN SELECT RAISE(ABORT, 'change events are immutable'); END;
CREATE TRIGGER change_events_no_delete BEFORE DELETE ON change_events BEGIN SELECT RAISE(ABORT, 'change events are immutable'); END;
CREATE TRIGGER objects_no_delete BEFORE DELETE ON objects BEGIN SELECT RAISE(ABORT, 'objects are retained'); END;
CREATE TRIGGER attachments_no_physical_delete BEFORE DELETE ON attachments BEGIN SELECT RAISE(ABORT, 'attachments are soft-delete only'); END;
CREATE TRIGGER edges_no_physical_delete BEFORE DELETE ON edges BEGIN SELECT RAISE(ABORT, 'edges are soft-delete only'); END;
CREATE TRIGGER citations_no_physical_delete BEFORE DELETE ON citations BEGIN SELECT RAISE(ABORT, 'citations are soft-delete only'); END;
CREATE TRIGGER block_citations_no_physical_delete BEFORE DELETE ON block_citations BEGIN SELECT RAISE(ABORT, 'block citations are soft-delete only'); END;
CREATE TRIGGER variables_no_physical_delete BEFORE DELETE ON variables BEGIN SELECT RAISE(ABORT, 'variables are soft-delete only'); END;
CREATE TRIGGER variable_definitions_no_physical_delete BEFORE DELETE ON variable_definitions BEGIN SELECT RAISE(ABORT, 'variable definitions are soft-delete only'); END;
CREATE TRIGGER variable_occurrences_no_physical_delete BEFORE DELETE ON variable_occurrences BEGIN SELECT RAISE(ABORT, 'variable occurrences are soft-delete only'); END;
CREATE TRIGGER snapshots_no_update BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT, 'snapshot records are append-only'); END;
CREATE TRIGGER snapshots_no_delete BEFORE DELETE ON snapshots BEGIN SELECT RAISE(ABORT, 'snapshot records are append-only'); END;
CREATE TRIGGER backups_no_update BEFORE UPDATE ON backups BEGIN SELECT RAISE(ABORT, 'backup records are append-only'); END;
CREATE TRIGGER backups_no_delete BEFORE DELETE ON backups BEGIN SELECT RAISE(ABORT, 'backup records are append-only'); END;
