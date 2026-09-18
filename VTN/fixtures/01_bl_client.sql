-- Bootstrap credential for the OpenADR 3.1 scope model.
--
-- This file creates exactly ONE user: the business-layer client the BFF and the seed
-- script authenticate as. Every other user -- the 20 fleet VEN credentials -- is created
-- through POST /users and POST /users/{id} by scripts/seed_vtn.py, so the VTN hashes each
-- secret itself and no argon2 hash is ever hand-maintained here. Only the bootstrap
-- credential has to exist before a token can be obtained, so only it lives in SQL.
--
-- Scope names are written IN FULL and deliberately. `write_vens`, `write_reports` and
-- `write_subscriptions` are accepted as aliases by the VTN, but each resolves to the *_ven
-- variant: a business client granted the alias would have its own token subject written
-- into every VEN object's clientID (openleadr-vtn/src/api/ven.rs), so the second VEN
-- creation would collide on ven_client_id_unique. See design.md D2.
--
-- write_vens_bl        -- create VEN objects carrying the VEN's own clientID
-- write_reports_bl     -- read and delete reports across the fleet
-- read_all             -- bypass target filtering; the BFF serves every UI view
-- write_programs/_events -- create and edit programs and events
-- write_users          -- provision the fleet's VEN users (needs the internal-oauth build)
--
-- The secret is "bl-client", matching the lab's existing convention of secret == client id
-- (as ven-1/ven-1 already does). The argon2id hash below is upstream's published test
-- fixture hash for that same string, so it is not a secret in any meaningful sense -- this
-- lab's VTN is not internet-exposed. Rotating it means replacing this hash only.

INSERT INTO "user" (id, reference, description, scopes, created, modified)
VALUES ('bl-client',
        'bl-client-ref',
        'Business-layer client: BFF proxy and seed script',
        '{"read_all", "write_programs", "write_events", "write_vens_bl", "write_reports_bl", "write_users"}',
        now(),
        now())
ON CONFLICT (id) DO UPDATE
    SET scopes   = excluded.scopes,
        modified = now();

INSERT INTO user_credentials (user_id, client_id, client_secret)
VALUES ('bl-client',
        'bl-client',
        '$argon2id$v=19$m=16,t=2,p=1$MWt1QVNFdHdlZVJhNEZzUA$Rmkguwgaz+A2GWIaDRtv8w')
ON CONFLICT (client_id) DO NOTHING;
