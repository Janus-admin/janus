-- Fix missing complexity_tier / quality_score for two models that were in
-- the catalogue but not explicitly scored in migration 0032.
-- Without this they fall back to tier='standard', score=5 — both deserve better.

-- llama-3.2-11b-vision-preview: a mid-size vision model, standard tier quality 6
UPDATE model_pricing
SET complexity_tier = 'standard',
    quality_score   = 6
WHERE model_id = 'llama-3.2-11b-vision-preview';

-- meta.llama3-70b-instruct-v1:0: Bedrock-hosted Llama 70B, same tier/score as its
-- peer meta.llama3-1-70b-instruct-v1:0 which was scored 6 in migration 0032.
UPDATE model_pricing
SET complexity_tier = 'standard',
    quality_score   = 6
WHERE model_id = 'meta.llama3-70b-instruct-v1:0';
