-- Dropped because a blank system_prompt is now a supported state: the shared
-- voice baseline rendered ahead of the persona in AGENTS.md carries the floor
-- (do not repeat verbatim / follow the reader's language / stay concise / take
-- a position), so an Agent created without a persona is still a usable
-- colleague rather than an engine default. The constraint forced every user to
-- write prose before the Agent could exist. See collaboration.md 3.4.
ALTER TABLE collab_agents
    DROP CONSTRAINT collab_agents_prompt_not_blank;

ALTER TABLE collab_runs
    ADD COLUMN outcome TEXT;

-- Runs created before outcome evidence was tracked cannot be classified honestly.
UPDATE collab_runs
   SET outcome = 'unknown'
 WHERE status = 'completed';

ALTER TABLE collab_runs
    ADD CONSTRAINT collab_runs_outcome_valid CHECK (
        outcome IS NULL OR outcome IN ('acted', 'silent', 'unpublished', 'unknown')
    ),
    ADD CONSTRAINT collab_runs_outcome_scope CHECK (
        (status =  'completed' AND outcome IS NOT NULL) OR
        (status <> 'completed' AND outcome IS     NULL)
    );
