import { Check, ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

export interface DropdownOption<T extends string> {
  value: T;
  label: string;
  description?: string;
  disabled?: boolean;
  icon?: ReactNode;
}

export function Dropdown<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "请选择",
  icon,
  className = "",
  align = "start",
}: {
  ariaLabel: string;
  value: T | "";
  options: DropdownOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  placeholder?: string;
  icon?: ReactNode;
  className?: string;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const matchingIndex = options.findIndex((option) => option.value === value);
  const firstEnabledIndex = options.findIndex((option) => !option.disabled);
  const selectedIndex = matchingIndex >= 0 && !options[matchingIndex]?.disabled
    ? matchingIndex
    : firstEnabledIndex;
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const listboxRef = useRef<HTMLDivElement>(null);
  const listboxId = useId();
  const selected = options.find((option) => option.value === value);
  const activeOptionId = activeIndex >= 0 && options[activeIndex]
    ? `${listboxId}-option-${activeIndex}`
    : undefined;

  const close = (restoreFocus = true) => {
    setOpen(false);
    if (restoreFocus) requestAnimationFrame(() => triggerRef.current?.focus());
  };
  const choose = (index: number) => {
    const option = options[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    close();
  };
  const nextEnabled = (start: number, direction: 1 | -1) => {
    if (!options.length) return -1;
    let index = start;
    for (let count = 0; count < options.length; count += 1) {
      index = (index + direction + options.length) % options.length;
      if (!options[index]?.disabled) return index;
    }
    return start;
  };

  useEffect(() => {
    if (!open) return;
    setActiveIndex(selectedIndex);
    requestAnimationFrame(() => listboxRef.current?.focus());
    const pointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) close(false);
    };
    window.addEventListener("pointerdown", pointer);
    return () => window.removeEventListener("pointerdown", pointer);
  }, [open, selectedIndex]);
  useEffect(() => {
    const option = optionRefs.current[activeIndex];
    if (open && typeof option?.scrollIntoView === "function") option.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  const onTriggerKeyDown = (event: React.KeyboardEvent) => {
    if (disabled) return;
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      setOpen(true);
      setActiveIndex(event.key === "ArrowUp" ? nextEnabled(selectedIndex, -1) : selectedIndex);
    }
  };
  const onListKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") { event.preventDefault(); close(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); setActiveIndex((index) => nextEnabled(index, 1)); }
    else if (event.key === "ArrowUp") { event.preventDefault(); setActiveIndex((index) => nextEnabled(index, -1)); }
    else if (event.key === "Home") { event.preventDefault(); setActiveIndex(options.findIndex((option) => !option.disabled)); }
    else if (event.key === "End") { event.preventDefault(); setActiveIndex(Math.max(0, [...options].map((option) => !option.disabled).lastIndexOf(true))); }
    else if (event.key === "Enter" || event.key === " ") { event.preventDefault(); choose(activeIndex); }
    else if (event.key === "Tab") close(false);
  };

  return <div ref={rootRef} className={`dropdown ${className} ${open ? "open" : ""}`}>
    <button
      ref={triggerRef}
      type="button"
      className="dropdown-trigger"
      aria-label={ariaLabel}
      aria-haspopup="listbox"
      aria-expanded={open}
      aria-controls={open ? listboxId : undefined}
      disabled={disabled}
      onClick={() => { if (open) close(); else setOpen(true); }}
      onKeyDown={onTriggerKeyDown}
    >
      {icon}{selected?.icon}<span className={!selected ? "placeholder" : ""}>{selected?.label ?? placeholder}</span><ChevronDown size={13}/>
    </button>
    {open && <div className={`dropdown-panel align-${align}`}>
      <div ref={listboxRef} id={listboxId} role="listbox" aria-label={ariaLabel} aria-activedescendant={activeOptionId} tabIndex={-1} onKeyDown={onListKeyDown}>
        {options.map((option, index) => <button
          key={option.value}
          id={`${listboxId}-option-${index}`}
          ref={(node) => { optionRefs.current[index] = node; }}
          type="button"
          role="option"
          aria-selected={option.value === value}
          disabled={option.disabled}
          className={`${index === activeIndex ? "active" : ""} ${option.value === value ? "selected" : ""}`}
          onMouseEnter={() => setActiveIndex(index)}
          onClick={() => choose(index)}
        >
          {option.icon}<span><strong>{option.label}</strong>{option.description && <small>{option.description}</small>}</span>{option.value === value && <Check size={14}/>}</button>)}
        {!options.length && <div className="dropdown-empty">没有可选项</div>}
      </div>
    </div>}
  </div>;
}
