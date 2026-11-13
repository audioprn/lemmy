-- audioprn: creator reports are filed against the `creators` community, so a person must be able
-- to report it more than once (one report per creator, and again later).
ALTER TABLE community_report
    DROP CONSTRAINT community_report_community_id_creator_id_key;
