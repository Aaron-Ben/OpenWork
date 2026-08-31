UPDATE models
SET config = config || jsonb_build_object(
        'capabilities',
        CASE model_name
            WHEN 'gpt-5.1' THEN jsonb_build_object(
                'contextWindowTokens', 400000,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', NULL,
                'acceptsDataBlocks', TRUE
            )
            WHEN 'claude-sonnet-4-5' THEN jsonb_build_object(
                'contextWindowTokens', 200000,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', NULL,
                'acceptsDataBlocks', TRUE
            )
            WHEN 'deepseek-v4-flash' THEN jsonb_build_object(
                'contextWindowTokens', 1048576,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', NULL,
                'acceptsDataBlocks', FALSE
            )
            WHEN 'kimi-k2.6' THEN jsonb_build_object(
                'contextWindowTokens', 262144,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', NULL,
                'acceptsDataBlocks', TRUE
            )
            WHEN 'qwen-plus' THEN jsonb_build_object(
                'contextWindowTokens', 1000000,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', 81920,
                'acceptsDataBlocks', FALSE
            )
            WHEN 'glm-5.2' THEN jsonb_build_object(
                'contextWindowTokens', 1000000,
                'maxOutputTokens', 32768,
                'maxReasoningTokens', NULL,
                'acceptsDataBlocks', FALSE
            )
        END
    ),
    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
WHERE NOT config ? 'capabilities'
  AND model_name IN (
      'gpt-5.1',
      'claude-sonnet-4-5',
      'deepseek-v4-flash',
      'kimi-k2.6',
      'qwen-plus',
      'glm-5.2'
  );
