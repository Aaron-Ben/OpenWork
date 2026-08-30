CREATE TABLE collab_boards (
    id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL REFERENCES collab_rooms(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
);
CREATE INDEX idx_collab_boards_room ON collab_boards(room_id);

CREATE TABLE collab_board_columns (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    position INTEGER NOT NULL CHECK (position >= 0),
    is_done BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT collab_board_columns_board_id_id_unique UNIQUE (board_id, id)
);
CREATE UNIQUE INDEX uq_collab_board_columns_position
    ON collab_board_columns(board_id, position);

CREATE TABLE collab_cards (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES collab_boards(id) ON DELETE CASCADE,
    column_id TEXT NOT NULL,
    title TEXT NOT NULL CHECK (btrim(title) <> ''),
    description TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    assignee_id TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_by TEXT REFERENCES collab_participants(id) ON DELETE SET NULL,
    claimed_at TIMESTAMP WITHOUT TIME ZONE,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
    CONSTRAINT collab_cards_column_in_board
        FOREIGN KEY (board_id, column_id)
        REFERENCES collab_board_columns(board_id, id)
        ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT collab_cards_claim_consistent CHECK (
        (claimed_by IS NULL AND claimed_at IS NULL) OR
        (claimed_by IS NOT NULL AND claimed_at IS NOT NULL)
    )
);
CREATE INDEX idx_collab_cards_column ON collab_cards(column_id, position);
CREATE INDEX idx_collab_cards_assignee
    ON collab_cards(assignee_id) WHERE assignee_id IS NOT NULL;
CREATE INDEX idx_collab_cards_claimed
    ON collab_cards(claimed_by, claimed_at) WHERE claimed_by IS NOT NULL;

ALTER TABLE collab_runs
    ADD COLUMN focus_card_id TEXT REFERENCES collab_cards(id) ON DELETE SET NULL,
    ADD COLUMN agenda_anchor_seq BIGINT,
    ADD COLUMN trigger_reason TEXT,
    ADD CONSTRAINT collab_runs_agenda_focus_shape CHECK (
        (trigger = 'agenda' AND room_id IS NOT NULL AND agenda_anchor_seq IS NOT NULL
            AND agenda_anchor_seq >= 0 AND trigger_reason IS NOT NULL
            AND btrim(trigger_reason) <> '') OR
        (trigger <> 'agenda' AND focus_card_id IS NULL AND agenda_anchor_seq IS NULL
            AND trigger_reason IS NULL)
    );
