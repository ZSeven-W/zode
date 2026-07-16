import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export function Markdown({ value }: { value: string }) {
  return (
    <div className="markdown-content">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        components={{
          a({ node: _node, href, ...props }) {
            const external = Boolean(href && !href.startsWith("#"));
            return (
              <a
                {...props}
                href={href}
                target={external ? "_blank" : undefined}
                rel={external ? "noopener noreferrer" : undefined}
              />
            );
          },
          img({ node: _node, ...props }) {
            return <img {...props} loading="lazy" decoding="async" />;
          },
          table({ node: _node, ...props }) {
            return (
              <div className="markdown-table-wrap">
                <table {...props} />
              </div>
            );
          },
          input({ node: _node, ...props }) {
            return <input {...props} disabled />;
          },
        }}
      >
        {value}
      </ReactMarkdown>
    </div>
  );
}
