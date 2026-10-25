import type { ComponentChildren } from "preact";
import { useEffect, useRef } from "preact/hooks";
import type { Option } from "../api";

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange(v: boolean): void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} class="toggle" onClick={() => onChange(!checked)}>
      <span class="toggle-thumb" />
    </button>
  );
}

export function Segmented({ value, options, onChange, label }: {
  value: string;
  options: Option[];
  onChange(id: string): void;
  label: string;
}) {
  return (
    <div class="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button type="button" role="radio" aria-checked={o.id === value} class="segment" onClick={() => onChange(o.id)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A labelled settings row: title and description on the left, the control on the right. */
export function Row({ title, description, children, htmlFor }: {
  title: string;
  description?: ComponentChildren;
  children: ComponentChildren;
  htmlFor?: string;
}) {
  return (
    <div class="row">
      <div class="row-text">
        {htmlFor ? <label class="row-title" for={htmlFor}>{title}</label> : <div class="row-title">{title}</div>}
        {description && <p class="row-description">{description}</p>}
      </div>
      <div class="row-control">{children}</div>
    </div>
  );
}

/** Native <dialog> confirm for the two irreversible actions (clear transcripts, reset stats). */
export function Confirm({ open, title, body, action, onConfirm, onCancel }: {
  open: boolean;
  title: string;
  body: string;
  action: string;
  onConfirm(): void;
  onCancel(): void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return (
    <dialog ref={ref} class="dialog" onClose={onCancel} aria-labelledby="dialog-title">
      <h2 id="dialog-title">{title}</h2>
      <p>{body}</p>
      <div class="dialog-actions">
        <button type="button" class="button" onClick={onCancel} autofocus>Cancel</button>
        <button type="button" class="button danger" onClick={onConfirm}>{action}</button>
      </div>
    </dialog>
  );
}
