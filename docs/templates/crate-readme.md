# 模板：crate README

本文规定 crate README 的结构与写法。每个 crate 在根目录有一份 `README.md`。

做法来自 DSH：模板见 `.agents/skills/dsh-doc/templates/package-reference.md` 与 `package-library.md`，模型体验的规定见 `docs/cookbook/adding-a-package.md` 第 4 节。与 DSH 的不同：只写中文；没有 `cordis.yml` 装配，“使用本 crate”写对外的入口。

## 1. 骨架

````markdown
---
description: "读者能用这个 crate 做什么。一到两句，带可搜索的领域词。"
---

# openwork-<name>

## 概述

三到五句：能用它做什么，什么时候选它，主要代价，最重要的边界。不写它“是什么类型”。

## 目录

- [使用本 crate](#use-this-crate)
- [理解实现](#understand-the-implementation)
- [进一步探索](#further-exploration)
- [模型体验](#model-experience)
- [已知限制](#known-limitations)

<a id="use-this-crate"></a>
## 使用本 crate

一句话说明最常见的用法。

### 何时选择

一段：选它或不选它的条件，以及不选时用什么。

### 入口与配置

列出对外的类型与函数，每个一句。有配置项时，加配置项表：

| 字段 | 默认值 | 含义 |
|---|---|---|
| `<field>` | `<默认值>` 或 `必填` | 一句话 |

<a id="understand-the-implementation"></a>
## 理解实现

<details>
<summary>实现细节——点击展开</summary>

设计理念、组件结构与数据流，够读者看懂即可。末尾用一张源码地图表链接文件。不复述 rustdoc。

</details>

<a id="further-exploration"></a>
## 进一步探索

三到七个相邻页面，先列最近的前提，每个一句。

<a id="model-experience"></a>
## 模型体验

### <一项进入模型上下文的内容>

#### 模型看到什么

数据决定的字段，或下面的原文。

##### <字段名>的原文

```markdown
从源码原样复制的稳定文本，例如系统提示。
```

#### Token 影响

固定、按条件出现、保留、替换、有上限，或没有直接影响。

#### KV Cache 影响

只追加、前缀稳定、替换已有内容，或独立请求。写明哪些本 crate 的改动会让缓存失效。

<a id="known-limitations"></a>
## 已知限制

- **使用者能看到的缺口** —— 缺什么，后果是什么，维护时的约束。

### 开发备注

<details>
<summary>维护者的工作背景——点击展开</summary>

无。

</details>
````

## 2. 规则

- **先核实，再写。** 每个配置项、默认值与行为，都要能在代码或测试中找到。找不到的内容删掉。
- 每一项进入模型上下文的内容写一个 H3。H3 下按顺序写三个 H4，每个 H4 一段。
- 不影响模型上下文的 crate，在“模型体验”下写一句“无”，或写“间接，经由 `<crate>`”。然后写“KV Cache 影响”一段。
- “已知限制”只写本 crate 持久的缺口。普通的清理工作写在代码 TODO 或 Agent Note 中。没有缺口时写“无”。
- 不复述子系统页的规则，链接它。决策理由链接 Agent Note。
- crate 的对外行为、配置或模型上下文改变时，在同一个改动里更新 README。
