ALTER TABLE community_report
    ADD CONSTRAINT community_report_community_id_creator_id_key UNIQUE (community_id, creator_id);
