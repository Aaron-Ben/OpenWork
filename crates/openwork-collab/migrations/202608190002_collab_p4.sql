-- P4 shared-board schema; the authoritative DDL is collaboration-data-model.md section 3.
CREATE TABLE collab_boards (
    id         TEXT PRIMARY KEY,
    room_id    TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_boards_title_not_blank CHECK (btrim(title) <> '')
);

CREATE INDEX idx_collab_boards_room ON collab_boards(room_id);

CREATE TABLE collab_board_columns (
    id       TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    title    TEXT NOT NULL,
    position INTEGER NOT NULL,
    is_done  BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT collab_board_columns_title_not_blank CHECK (btrim(title) <> '')
);

CREATE UNIQUE INDEX uq_collab_board_columns_position
    ON collab_board_columns(board_id, position);

CREATE TABLE collab_cards (
    id          TEXT PRIMARY KEY,
    board_id    TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    column_id   TEXT NOT NULL REFERENCES collab_board_columns(id) ON DELETE RESTRICT,
    title       TEXT NOT NULL,
    description TEXT,
    position    INTEGER NOT NULL,
    assignee_id TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_by  TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_at  TIMESTAMP WITHOUT TIME ZONE,
    created_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at  TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_cards_title_not_blank CHECK (btrim(title) <> ''),
    CONSTRAINT collab_cards_claim_consistent CHECK (
        (claimed_by IS     NULL AND claimed_at IS     NULL) OR
        (claimed_by IS NOT NULL AND claimed_at IS NOT NULL)
    )
);

CREATE INDEX idx_collab_cards_column ON collab_cards(column_id, position);
CREATE INDEX idx_collab_cards_assignee
    ON collab_cards(assignee_id) WHERE assignee_id IS NOT NULL;
