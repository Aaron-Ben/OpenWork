-- Column 的语义由可空的 kind（todo / doing / done）表达，替换只能区分终态的 is_terminal
-- （collaboration.md §11.1）。
--
-- 回填口径：is_terminal 为真的列是 done；其余列只有 Board 创建时自带的 "Todo" / "Doing"
-- 两个标题能确定语义，按标题回填；其他列无法判断，保持未分类（NULL），领取不会移动其中的卡片。
-- 删除 is_terminal：语义已完整转入 kind，不保留两份。
ALTER TABLE collab_board_columns
    ADD COLUMN kind TEXT CHECK (kind IN ('todo', 'doing', 'done'));

UPDATE collab_board_columns
SET kind = CASE
    WHEN is_terminal THEN 'done'
    WHEN title = 'Todo' THEN 'todo'
    WHEN title = 'Doing' THEN 'doing'
END;

ALTER TABLE collab_board_columns DROP COLUMN is_terminal;
