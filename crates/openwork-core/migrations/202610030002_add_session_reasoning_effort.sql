-- Session 选的推理档位，即 Responses `reasoning.effort` 的取值。
-- NULL 表示使用模型目录中的默认档位。取值由模型目录校验，不加 CHECK 约束：各模型的档位不同。
-- 设计见 .agents/notes/proposed/simplification/2026-10-03-responses-only-model-access.md。

ALTER TABLE sessions ADD COLUMN reasoning_effort TEXT;
