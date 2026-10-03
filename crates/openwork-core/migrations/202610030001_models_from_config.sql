-- Provider 与模型改由 ~/.openwork/config.json 和打包的模型目录提供，不再存库。
-- 设计见 .agents/notes/proposed/simplification/2026-10-03-responses-only-model-access.md。
--
-- 破坏性操作的理由：
-- 1. 删除 models 与 provider_credentials。现有数据不迁移（用户决定，2026-10-03）。
-- 2. sessions、turns、trace_spans 的 model_id 改为模型引用 `<providerId>/<modelId>` 的文本，
--    指向配置文件中的条目，所以去掉外键。旧值是 `model:<provider>:<model>`，不再能解析。
-- 3. turns.resolved_provider_kind 改名为 resolved_provider_id：线协议只剩 Responses，
--    记录的是这次调用使用的 Provider 配置 id，不再是厂商种类。

ALTER TABLE sessions DROP CONSTRAINT sessions_default_model_id_fkey;
ALTER TABLE turns DROP CONSTRAINT turns_model_id_fkey;
ALTER TABLE trace_spans DROP CONSTRAINT trace_spans_model_id_fkey;

ALTER TABLE turns RENAME COLUMN resolved_provider_kind TO resolved_provider_id;

DROP TABLE models;
DROP TABLE provider_credentials;
