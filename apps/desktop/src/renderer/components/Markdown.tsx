import ReactMarkdown, { type Components, type ExtraProps } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";

/** 围栏代码块的语言，来自 `language-xxx` 类名。 */
function codeLanguage(node: ExtraProps["node"]): string | undefined {
  const code = node?.children[0];
  if (code?.type !== "element") return undefined;
  const classes = code.properties.className;
  if (!Array.isArray(classes)) return undefined;
  const language = classes.map(String).find((name) => name.startsWith("language-"));
  return language?.slice("language-".length);
}

const components: Components = {
  // 链接一律在新窗口打开；主进程把新窗口请求交给系统浏览器（main/navigation.ts）。
  a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noreferrer" />,
  pre: ({ node, children, ...props }) => {
    const language = codeLanguage(node);
    return (
      <pre {...props}>
        {language && (
          <span className="absolute top-2 right-3 font-mono text-[11px] text-faint select-none">{language}</span>
        )}
        {children}
      </pre>
    );
  },
};

/**
 * 消息正文按 Markdown 渲染。不渲染原始 HTML，`javascript:` 等链接由 react-markdown 默认过滤。
 * 只给写了语言的代码块着色，不自动猜语言。
 */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="prose">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: false }]]}
        components={components}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
