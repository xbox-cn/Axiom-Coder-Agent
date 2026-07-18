import { ChevronRight, FileDiff } from "lucide-react";
import { memo, useMemo, useState } from "react";

interface DiffSection {
  key: string;
  path: string;
  lines: string[];
  additions: number;
  deletions: number;
}

function sectionPath(lines: string[], fallback: string) {
  const target = lines.find((line) => line.startsWith("+++ "))?.slice(4).trim();
  if (target && target !== "/dev/null") return target.replace(/^b\//, "");
  const source = lines.find((line) => line.startsWith("--- "))?.slice(4).trim();
  if (source && source !== "/dev/null") return source.replace(/^a\//, "");
  const header = lines[0]?.match(/^diff --git a\/(.+) b\/(.+)$/);
  return header?.[2] ?? fallback;
}

function parseDiff(diff: string): DiffSection[] {
  if (!diff.trim()) return [];
  const sections: string[][] = [];
  let current: string[] = [];
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ") && current.length) {
      sections.push(current);
      current = [];
    }
    current.push(line);
  }
  if (current.length) sections.push(current);
  return sections.map((lines, index) => ({
    key: `diff-${index}-${sectionPath(lines, `文件 ${index + 1}`)}`,
    path: sectionPath(lines, `文件 ${index + 1}`),
    lines,
    additions: lines.reduce((count, line) => count + Number(line.startsWith("+") && !line.startsWith("+++")), 0),
    deletions: lines.reduce((count, line) => count + Number(line.startsWith("-") && !line.startsWith("---")), 0),
  }));
}

function lineKind(line: string) {
  if (line.startsWith("+") && !line.startsWith("+++")) return "add";
  if (line.startsWith("-") && !line.startsWith("---")) return "remove";
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("diff ") || line.startsWith("index ") || line.startsWith("+++") || line.startsWith("---")) return "header";
  return "context";
}

function DiffView({ diff }: { diff: string }) {
  const sections = useMemo(() => parseDiff(diff), [diff]);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(sections[0] ? [sections[0].key] : []));

  if (!sections.length) return <div className="no-diff">未检测到文本 Diff；未跟踪文件仍会列在上方。</div>;

  const toggle = (key: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  return (
    <div className="diff-view">
      {sections.map((section) => {
        const open = expanded.has(section.key);
        return (
          <section className={`diff-file ${open ? "open" : ""}`} key={section.key}>
            <button className="diff-file-header" onClick={() => toggle(section.key)} aria-expanded={open}>
              <ChevronRight size={14} />
              <FileDiff size={14} />
              <span title={section.path}>{section.path}</span>
              <small className="diff-additions">+{section.additions}</small>
              <small className="diff-deletions">−{section.deletions}</small>
            </button>
            {open ? (
              <div className="diff-file-lines">
                {section.lines.map((line, index) => <div key={`${section.key}-${index}`} className={`diff-line ${lineKind(line)}`}><span className="line-number">{index + 1}</span><code>{line || " "}</code></div>)}
              </div>
            ) : null}
          </section>
        );
      })}
    </div>
  );
}

export default memo(DiffView);
