-- audioprn: blocking works both ways, like Reddit. When A blocks B, B's row for A gets
-- blocked_by_at, so everything that hides blocked people from A (filter_blocked) also hides A
-- from B. Admins and moderators are never hidden from (they must see everything to moderate).
-- Blocking also ends follows both ways.
ALTER TABLE person_actions
    ADD COLUMN blocked_by_at timestamptz;

CREATE FUNCTION audioprn_is_staff (p int)
    RETURNS bool
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        EXISTS (
            SELECT 1 FROM local_user WHERE person_id = p AND admin)
        OR EXISTS (
            SELECT 1 FROM community_actions WHERE person_id = p AND became_moderator_at IS NOT NULL)
$$;

-- True when either person blocked the other: they don't see each other.
CREATE FUNCTION audioprn_blocked (viewer int, other int)
    RETURNS bool
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        EXISTS (
            SELECT 1 FROM person_actions
            WHERE person_id = viewer AND target_id = other
                AND (blocked_at IS NOT NULL OR blocked_by_at IS NOT NULL))
$$;

CREATE FUNCTION audioprn_mirror_block ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
DECLARE
    blocker int;
    blocked int;
    blocking bool;
BEGIN
    IF TG_OP = 'DELETE' THEN
        -- Unblocking deletes a row once its own columns are empty: keep the other person's block.
        IF OLD.blocked_by_at IS NOT NULL AND EXISTS (SELECT 1 FROM person WHERE id = OLD.person_id)
            AND EXISTS (SELECT 1 FROM person WHERE id = OLD.target_id) THEN
            INSERT INTO person_actions (person_id, target_id, blocked_by_at)
                VALUES (OLD.person_id, OLD.target_id, OLD.blocked_by_at)
            ON CONFLICT (person_id, target_id)
                DO UPDATE SET blocked_by_at = excluded.blocked_by_at;
        END IF;
        IF OLD.blocked_at IS NULL THEN
            RETURN NULL;
        END IF;
        blocker := OLD.person_id;
        blocked := OLD.target_id;
        blocking := FALSE;
    ELSE
        IF (TG_OP = 'INSERT' AND NEW.blocked_at IS NULL)
            OR (TG_OP = 'UPDATE' AND (OLD.blocked_at IS NULL) = (NEW.blocked_at IS NULL)) THEN
            RETURN NULL;
        END IF;
        blocker := NEW.person_id;
        blocked := NEW.target_id;
        blocking := NEW.blocked_at IS NOT NULL;
    END IF;
    IF blocking THEN
        UPDATE person_actions SET followed_at = NULL, follow_pending = NULL
        WHERE (person_id, target_id) IN ((blocker, blocked), (blocked, blocker));
        IF NOT audioprn_is_staff (blocked) THEN
            INSERT INTO person_actions (person_id, target_id, blocked_by_at)
                VALUES (blocked, blocker, now())
            ON CONFLICT (person_id, target_id)
                DO UPDATE SET blocked_by_at = now();
        END IF;
    ELSE
        UPDATE person_actions SET blocked_by_at = NULL
        WHERE person_id = blocked AND target_id = blocker;
        DELETE FROM person_actions
        WHERE person_id = blocked AND target_id = blocker
            AND followed_at IS NULL AND blocked_at IS NULL AND blocked_by_at IS NULL
            AND noted_at IS NULL AND voted_at IS NULL;
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_mirror_block
    AFTER INSERT OR DELETE OR UPDATE OF blocked_at ON person_actions
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_mirror_block ();

-- Existing blocks.
INSERT INTO person_actions (person_id, target_id, blocked_by_at)
SELECT target_id, person_id, blocked_at
FROM person_actions
WHERE blocked_at IS NOT NULL AND NOT audioprn_is_staff (target_id)
ON CONFLICT (person_id, target_id)
    DO UPDATE SET blocked_by_at = excluded.blocked_by_at;

UPDATE person_actions f SET followed_at = NULL, follow_pending = NULL
FROM person_actions b
WHERE b.blocked_at IS NOT NULL AND f.followed_at IS NOT NULL
    AND ((f.person_id, f.target_id) IN ((b.person_id, b.target_id), (b.target_id, b.person_id)));
