-- ============================================================================
-- SITUATION ROOM: Situation Cleanup SQL
-- Generated: 2026-03-22
--
-- This script removes noise, natural disasters, duplicates, and off-topic
-- situations to bring the top-level count from ~222 down to ~30.
--
-- Strategy: Use recursive CTE to find all descendants of each target,
-- then delete from FK-dependent tables first (NO ACTION constraints),
-- then delete the situations themselves (CASCADE handles situation_events
-- and situation_entities automatically).
--
-- DRY RUN: Change COMMIT to ROLLBACK at the bottom to preview.
-- ============================================================================

BEGIN;

-- Step 1: Collect all situation IDs to delete (targets + all descendants)
CREATE TEMP TABLE situations_to_delete AS
WITH RECURSIVE tree(id) AS (
    -- Seed: all top-level situations we want to remove
    SELECT id FROM situations WHERE id IN (

        -- ========== EARTHQUAKES (10) ==========
        '6cdd6c49-e60d-4eec-a4de-61c0bbee2fef',  -- California Earthquake Swarm
        'f8ee28f0-d9e3-409f-b96c-4cc91ed37cc0',  -- California Earthquake Swarm (dup)
        'd924b5bf-d117-47f9-aa35-36ead801be30',  -- East Asia Earthquakes
        'ee1914d1-fb1f-4cb0-8999-72e40d6953fd',  -- Tonga Samoa Earthquake Swarm
        '988bd26a-dd99-48a7-8460-bbfcf9416faf',  -- Greece Earthquakes
        '19dfecfb-cf09-4542-a96c-a8973d40a5dc',  -- Mariana Islands Earthquake
        '894214e3-ba68-4d4f-8d59-40035914932c',  -- Kermadec Islands Earthquakes
        '92bc0d97-c0e2-4642-bfbb-a3740139cf9f',  -- Peru Earthquake and Drought
        '2bae52d4-dee0-4deb-ba60-c0a8b16cbe40',  -- South Shetland Islands Earthquakes
        '6fcbfd54-8685-43a9-bc3e-125052f42ff2',  -- Chile Earthquake Strikes
        '99ce9549-2e1a-47f3-9e51-40a4630863bd',  -- Bolivia Earthquake

        -- ========== WILDFIRES / FIRE CLUSTERS (5) ==========
        '9a93760b-02a0-4ba7-91fa-d1e6ded90784',  -- South Asia Wildfires
        '6c8e7622-8232-4ce6-97b5-330a3405a4e0',  -- Sub-Saharan Africa Wildfires
        '2f425047-cb99-4b4e-8298-531003c34ac1',  -- East Asia Fire Clusters
        'fbc1fd27-eb5f-4feb-bd3b-c970c36e84eb',  -- Southeast Asia Wildfires
        '0cbb43a2-71fa-4777-89e4-148808e08a18',  -- Sub-Saharan Africa Wildfires (dup)

        -- ========== WEATHER / NATURAL DISASTERS (9) ==========
        'fb0da3b5-d048-490b-af9e-82cbe90b0080',  -- South Asia Drought
        '1732c3a1-8b9b-4847-9593-b16fbfda7b71',  -- Philippines Volcanic Eruption
        '3025ddc2-8b46-42c0-9063-db93ab65a35a',  -- North America Drought
        '9381c958-e22c-451c-8c66-1b438290d12e',  -- Caribbean Hurricane Beryl
        '03ad3e09-8159-4375-9f49-8f78a603d1c6',  -- Hurricane Beryl Caribbean Surge (dup)
        '3c42dd98-3ac4-43a2-8963-01f5680f0329',  -- Spain Storm Kristin Flooding
        '4e29e7d3-5ca9-4228-95e3-29bcf888c540',  -- East Africa Floods and Cyclones
        'ef6155c4-a805-4bcb-952c-3535079e899d',  -- NOAA CPC Weather Alerts
        '72eca708-c171-4abe-b683-4a3bcd49cc9c',  -- Republic of Congo Cholera Outbreak

        -- ========== SPORTS / ENTERTAINMENT (3) ==========
        '0f61d746-67d1-4e7e-b5ea-c8a7bccfc363',  -- Congo-Nigeria FIFA Sanctions
        '04e38740-395c-4201-8d0d-e19b943cd281',  -- Canada Postpones WAFCON 2026
        'b9cabeb5-20c3-4379-95b7-49f0aaa9dabb',  -- South Korea Defeated by Japan

        -- ========== NOISE / GOSSIP / INDIVIDUALS / MALFORMED (28) ==========
        '131ae39e-cf33-40df-9f74-a138d187ff2a',  -- Roman Calendar Reform Explained
        '71840ba4-3ffe-44ef-9dd9-6ceb57089192',  -- Bret Devereaux Blog Updates
        '7e57f107-793c-4500-9514-8997f3415cce',  -- Starfire Podcast Political Commentary
        'dd8362a6-594e-4cf0-bec3-8cd699d926e1',  -- Tony Stark PRC Conflict Analysis
        '173e5454-f595-4359-b462-4832c5df8ec9',  -- Brad Duplessis Under Fire
        '31a010d3-dc53-4e47-8630-9e8fb2d4251c',  -- Sophie Duchess of Edinburgh Visits
        'f5fc03b8-d9c4-4dc1-8ff8-6f4876b35cba',  -- Putins Wife Lyudmila
        'fb12e486-4527-4c0f-9464-932f6f07171f',  -- Eric L. Robinson Arrested
        'cbc310f6-f1e1-4d7c-b96a-f3ce524deeb4',  -- Cameron Abadi Arrested
        '026fdc7c-510f-4d5a-8779-dfbe876f3cf0',  -- Masood Masjoody Arrested
        '688ce5be-34b1-4bfc-8710-5afddf48e379',  -- Boris Pistorius Arrested
        'c299b0f7-cfb5-4f85-b40c-188a994f60b1',  -- Ghislaine Maxwell Trial Sentences
        '96be1a14-9f62-4c9f-a51d-cb7e7cc928a2',  -- Prince Andrew Scandal
        'ab70ffa5-019e-42f4-a1bc-778082e459fb',  -- Prince Andrew Arrested
        '2efec1fd-3d59-444e-98a7-3e6d757927b8',  -- Glaxton EnoughWolf Inventory Crisis
        '0b78e1e0-4d89-45aa-8a9f-df3834da57ea',  -- Daedeok Gu Tech Park Expansion
        'b0c1fd0c-079f-46bf-b5e0-0724c106f15b',  -- North America Online Debate
        '3886ea7c-695b-48fe-8cc2-51ae26ea5c96',  -- US Social Media Addiction Trial
        'b4159bad-4566-4cfd-b2a0-b52420e7c5d0',  -- US Air Force Academy Appointment
        '40e07088-f8b6-4022-89f4-fba7bd7bc841',  -- Seoul Stock Market Plunge
        'c3fa0690-f75d-4653-8565-bf7ffe3097e5',  -- US Dollar Market Volatility
        'cd389bc0-e999-4dcb-8835-c0c4d672600f',  -- China Renminbi Devaluation
        '5a2cdc2f-0baf-48da-8a0a-942eebaaf057',  -- North America Tech Layoffs
        '3f274beb-fbfc-45ea-964c-7f7012b4ce49',  -- North America Republican Party Surge
        '8b7f2ea8-d1eb-4b65-8f7b-4de2899e9f13',  -- US School Shooting
        '6495f822-eaa0-4467-8c60-de57fb34ceb2',  -- Toronto Police Service Arrests
        '1955a048-e31f-4b82-b443-2541db0beceb',  -- Brazilian Senate Vote
        '791183d7-d516-4707-87a2-451e47a57620',  -- Hoosier Homer Video Project
        'ccf506da-5101-4387-8793-d814c4c18c38',  -- International Paralympic Committee Meeting

        -- ========== ALGAL BLOOM (not geopolitical) (2) ==========
        '34394291-0235-40d0-af41-41c111af4fd4',  -- Iran Crimson Tide Spectacle
        '3c19fa4a-7835-4ec7-a56a-30ff563398e4',  -- Iran Hormuz Island Crimson Tide

        -- ========== DOMESTIC POLITICS (not OSINT-relevant) (18) ==========
        'd4d669fd-4e68-4f8c-a6c1-44bf80b6ce03',  -- Sweden Election Results
        '337fb7a8-d32b-48fb-a139-861efe817fac',  -- Spain Government Reshuffle
        'd4393c8d-3a28-46ea-bba5-cd34a94b56f1',  -- Spain PM Rejects War Threat
        '7d1712cd-af8a-4f39-9ef8-51d0bf6bbb99',  -- Spain Court Rejects Flood Indictment
        '19d3c096-6bb2-4ac6-b7aa-72befddc4572',  -- Anthony Albanese Australia Election
        'be1201cd-b8e7-46a1-a28d-131071dcf142',  -- Angus Taylor Investigation
        '78ff2c58-5695-4c9c-9395-baa34249bfae',  -- Slovenia Government Shutdown
        '60791b09-4984-4bc9-87a6-b97c0c501d54',  -- Ghana Investment Reset
        '407ecdad-2fc0-432b-bdc1-23c609f2b9da',  -- South Korea Industry Minister Resigns
        '3443490d-01e6-4a07-85c6-c6535b718a61',  -- Poland Returns Nazi-Stolen Artifacts
        'a4f2208d-c9e0-4bf8-8ac1-20c328b92deb',  -- Brazil Deforestation Surge
        'e8092b53-5c28-4243-b853-99bef5b56a15',  -- Trump US Election Campaign
        'a71f2699-34e9-4efc-b301-a3675b4285a7',  -- Trump Ownership Claims
        'cdaf7f35-9933-474d-bc4e-e8b0a9b4d31d',  -- US Political Rhetoric
        '234d5120-edfd-4697-979c-1e856785ccfc',  -- France Political Scandal
        '4553d2f6-5b6a-403b-aacd-aa817d042583',  -- Paris Mayoral Race
        '435bc15d-f224-40ba-a9df-497fef6b4113',  -- Germany CDU Election Competition
        '8264b18c-bf2f-4cc7-a936-6a48e11820d7',  -- US Political Culture Crisis

        -- ========== RESOLVED PAST EVENTS / STALE (14) ==========
        'c1e959b5-f9d6-44c2-9b62-18d92ee02e69',  -- Iran President Raisi Death
        'e86ee44f-603a-4aab-8948-f0f79da4aba7',  -- Iran Leadership Crisis
        'adbb2ece-373d-4820-9777-ca5aeaf7c3f5',  -- EU Iran Gas Price Surge
        '8bd0a15a-e046-4357-84f4-424208062115',  -- Turkey Recognizes Macedonia
        '9601c292-b259-43ba-8377-c0fdc9264764',  -- Israel Cyber Attacks (resolved, vague)
        '9000db50-cc79-4ae7-990b-0227c8936a9a',  -- East Asia Tech Hackers
        'c3ed91ff-faf6-495d-9d18-e290579fa2f8',  -- East Asia LNG Tanker Disruptions
        'be28e218-93d9-437c-b53d-22fce50dcb02',  -- Amsterdam Jewish School Attack
        '9a6df787-c33c-41b2-b7d3-683e3a9e9f5f',  -- Western Balkans Political Standoff
        '62e37356-a582-4868-a871-805386762a68',  -- Spain Rebuts Trump Trade Threats
        'd6d3e128-c787-4a19-ba4b-907082aa3ae1',  -- Nigeria Temu Privacy Investigation
        'b0f39d67-7bdd-4954-8fd3-59d7d25ba232',  -- US Tariff Uncertainty (dup of US-China Trade)
        'a9fb7a84-f6ff-48e9-86f5-26ef30bba039',  -- Poland Military Flights
        'e4d37ce7-9022-42b9-94c6-c959a7c28e5f',  -- Russian National Arrests

        -- ========== VAGUE META-SITUATIONS (3) ==========
        'dfad228e-3ab2-49c9-93b2-b83557b6acd0',  -- Middle East Monitor Updates
        'dc640dcf-6c9a-4304-a50b-cf7ca0fac076',  -- Sri Lanka Situation Reports
        'a1bf85c3-6618-4ce0-86c5-f46bfe202b91',  -- Geneva Diplomatic Talks

        -- ========== DUPLICATES (keep one from each pair) (7) ==========
        '831cc8fe-59d8-4829-9dc5-3f90090b895d',  -- Canada Consulate Shooting Probe (keep Investigation)
        '9d9a06e5-c119-4316-89d7-92515a9d4d7a',  -- APT28 Exploits MSHTML Zero-Day (keep 0-Day)
        '24755c2b-989e-4860-bbcb-fd13f0f54b22',  -- France Nuclear Arsenal Expansion (keep Boost)
        '45f2363d-bba9-4975-a205-76ce1280f79a',  -- Kosovo Political Instability (keep Governance Crisis)
        '4861759e-bb78-4431-8738-bcdd2616caf6',  -- Cybersecurity Attacks Target Sign-Up Pages (keep SMS Pumping)
        '1a0e672a-9fb7-413f-8877-14564f2ec739',  -- Azerbaijan Spy Conviction (keep the other)
        '930605ba-86da-4bce-bda4-fad2d6ef71de',  -- Western Europe Cable Disruption (keep Power Cable)

        -- ========== MORE NOISE (22) ==========
        '94524ae8-0134-450c-8a4e-ae1ee020dc28',  -- Travis Reese NATO War Criticism
        '3c0509f5-1a9c-4529-adab-5c1057a3611e',  -- Swiss Bus Fire Probe
        'c0be2444-f15f-4848-9603-b84591c1d888',  -- Pentagon Anthropic AI Showdown
        '667201cb-fc52-4307-8acb-2caaa90fdb0d',  -- Microsoft Teams Bot Security Updates
        'd12dd4b1-63a2-4fd1-950d-1c588e881d12',  -- Marco Rubio Political Maneuvers
        'eaec4c7f-3a6e-44b0-a47e-6326730a67d5',  -- Paris Security Alert
        'b169ae9f-2e4b-4f69-9bc2-b750c9086f61',  -- France Diplomatic Push
        '0313e8f8-be42-4be3-bf8e-c65c167d46c4',  -- UN Development Summit
        '8a34d59f-ccca-4d66-9165-adf87edbb8b3',  -- UN Global Summit
        'c50e9bf2-3358-4aae-9902-1ed332709456',  -- UN East Asia Summit
        '12e3ab07-1e42-4ac7-8328-cea4895b2037',  -- Brussels EU Summit
        '8a9a6dba-937b-4d53-98f6-2268d72b76f0',  -- Gulf Cooperation Council Summit
        'c8dba93e-f3f8-4073-9181-d4f3258df331',  -- World Nuclear Association Summit
        'd513296d-ecc8-4609-b7ce-43e8502aaae7',  -- US Consulate Toronto Closure
        'a04ec251-bb7c-4e58-aa6b-437c58758cc4',  -- France Sarkozy Trial Opens
        '6f7f5b42-6a70-4e8d-8459-28bcd2e3f7ba',  -- France Artillery Lessons
        'bd4f0702-5ecb-4280-8c9b-f316b0fbed82',  -- Eliot Higgins Africa Investigation
        'a784ad37-3788-413f-8a3a-efccba5c36ad',  -- UK Mandelson Files Released
        '6b060cff-ac9e-4ca3-af05-3f8d4e658a27',  -- Sri Lanka Humanitarian Crisis (4 events)
        '9c7e3e2e-711d-470d-b4f9-058692bcc2f5',  -- Data Exfiltration in Unknown Region (3 events)
        '1ae5df95-8dad-4214-a963-0e106b0e6b10',  -- Dubai Civil Defence Fire Response
        'b6fdddd2-5c36-4ee2-8a69-4f73e2f2b87b',  -- Russia Kenya Recruit Deal
        'f74afd06-b23e-4cd4-a93f-ddd80717915a',  -- Iran International Broadcasts
        'c342e814-e8bf-491d-b745-9d350bb4f271',  -- US Allies Coordinate Regional Response
        '3f1e0364-3c9e-4fc0-9a22-7e2af826cc70',  -- Krasnodar Region Oil Reserves
        'bde483f7-15de-4fa0-a486-b4d7378dc290',  -- US Undersecretary Asia Visit
        'f04bc043-91ee-4cbb-9129-7e71871ea576',  -- Somalia Journalist Killed
        '19eff283-bf18-42da-bba3-518b7776549e',  -- India Nuclear Device Recovery
        '0ee74a1c-2122-4f9b-86cb-03a6f9ded7dc',  -- Brazil Uranium Concerns
        '2119cbb1-1293-43bd-8503-0966b0d5c969',  -- London Al-Quds Day Arrests
        '6b075f92-0a0a-4900-8aa7-cc3cdff84fc3',  -- US Airport Shutdown Crisis
        'a2a7afbb-33b2-4961-863d-d15c6034ed99',  -- Beijing Palmer Trade Shifts
        '389f1964-97f0-4339-84e4-d24c24d68906',  -- Canada Economic Policy Shifts
        'e5537d66-b6c2-45a2-96c0-c66c915e2f31',  -- Japan Okinawa Boat Capsizing
        '1f98fb57-5d6b-4701-bd84-08b96dfbc30c',  -- East Asia Matsu Activity
        '6d30c0f6-ee7e-46aa-bb3e-d746aac1fab8'   -- US Airline CEOs Urge Congress (mutated title)
    )
    UNION ALL
    -- Recurse: find all children, grandchildren, etc.
    SELECT s.id FROM situations s
    JOIN tree t ON (s.properties->>'parent_id')::uuid = t.id
)
SELECT DISTINCT id FROM tree;

-- Report what we're about to delete
SELECT 'Situations to delete: ' || count(*) as info FROM situations_to_delete;

-- Step 2: Delete from FK-dependent tables (NO ACTION constraints)
DELETE FROM situation_phase_transitions WHERE situation_id IN (SELECT id FROM situations_to_delete);
DELETE FROM situation_narratives WHERE situation_id IN (SELECT id FROM situations_to_delete);
DELETE FROM situation_summaries WHERE situation_id IN (SELECT id FROM situations_to_delete);
DELETE FROM situation_timeline WHERE situation_id IN (SELECT id FROM situations_to_delete);
-- alert_history and intel_reports have no matching rows, but clean anyway
DELETE FROM alert_history WHERE situation_id IN (SELECT id FROM situations_to_delete);

-- Step 3: Delete situations (CASCADE handles situation_events + situation_entities)
DELETE FROM situations WHERE id IN (SELECT id FROM situations_to_delete);

-- Step 4: Clean up parent_id references in remaining situations
-- (children whose parents were deleted should become top-level)
UPDATE situations
SET properties = properties - 'parent_id'
WHERE (properties->>'parent_id') IS NOT NULL
  AND (properties->>'parent_id')::uuid NOT IN (SELECT id FROM situations);

-- Step 5: Clean up child_ids references in remaining situations
-- (remove deleted children from parent's child_ids array)
-- This is harder with JSONB arrays, but we can rebuild them
UPDATE situations s
SET properties = jsonb_set(
    s.properties,
    '{child_ids}',
    COALESCE(
        (SELECT jsonb_agg(cid)
         FROM jsonb_array_elements_text(s.properties->'child_ids') cid
         WHERE cid::uuid IN (SELECT id FROM situations)),
        '[]'::jsonb
    )
)
WHERE s.properties ? 'child_ids'
  AND jsonb_array_length(s.properties->'child_ids') > 0;

-- Report results
SELECT 'Remaining top-level situations: ' || count(*) as info
FROM situations WHERE properties->>'parent_id' IS NULL;

SELECT 'Total situations remaining: ' || count(*) as info FROM situations;

DROP TABLE situations_to_delete;

-- ============================================================================
-- CHANGE TO COMMIT WHEN READY TO EXECUTE FOR REAL
-- ============================================================================
ROLLBACK;
