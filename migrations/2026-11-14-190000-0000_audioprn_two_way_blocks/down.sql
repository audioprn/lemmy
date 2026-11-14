DROP TRIGGER audioprn_mirror_block ON person_actions;

DROP FUNCTION audioprn_mirror_block, audioprn_blocked, audioprn_is_staff;

ALTER TABLE person_actions
    DROP COLUMN blocked_by_at;
