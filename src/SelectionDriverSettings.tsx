import { useRef, useState } from "react";
import { AssistantSettings, selectionApi, selectionError } from "./selection";

export default function SelectionDriverSettings({ assistant, onSaved, inline = false }: {
  assistant?: AssistantSettings;
  onSaved: (settings: AssistantSettings) => void;
  inline?: boolean;
}) {
  const [apiKey, setApiKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const keyRef = useRef<HTMLInputElement>(null);
  const masked = Boolean(assistant?.hasCredential && !replacing);

  async function save() {
    if (masked || !apiKey.trim() || saving) return;
    setSaving(true); setMessage(""); setFailed(false);
    try {
      const settings = await selectionApi.saveAssistant(apiKey.trim());
      setApiKey(""); setReplacing(false); onSaved(settings); setMessage("已保存。");
    } catch (cause) { setFailed(true); setMessage(selectionError(cause)); }
    finally { setSaving(false); }
  }

  return <section className={`selection-driver-settings${inline ? " is-inline" : ""}`} aria-label="模型选型驱动 AI">
    {!inline && <h3>模型选型驱动 AI</h3>}
    <form className="selection-driver-row" onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <span className="selection-driver-model">{inline ? "配置模型选型驱动 AI" : "DeepSeek V4.1 Flash 1017"}</span>
      <div className={`credential-field selection-driver-key${masked ? " is-saved" : ""}`}>
        <input ref={keyRef} aria-label="官网 API Key" type="password" value={masked ? "••••••••••••••••••••••••••••••••" : apiKey} readOnly={masked} autoComplete="off" autoCapitalize="off" spellCheck={false} maxLength={4096} disabled={saving} placeholder="粘贴 DeepSeek 官网 API Key" onChange={(event) => { setApiKey(event.target.value); setMessage(""); }} onBlur={() => { if (!apiKey.trim()) setReplacing(false); }} />
        {masked && !inline && <button className="credential-replace" type="button" onClick={() => { setReplacing(true); setMessage(""); keyRef.current?.focus(); }}>更换</button>}
      </div>
      {!masked && <button className="primary-button" type="submit" disabled={saving || !apiKey.trim()}>{saving ? "保存中…" : "保存 API Key"}</button>}
    </form>
    {message && <p className={failed ? "selection-error" : "selection-notice"} role={failed ? "alert" : "status"}>{message}</p>}
  </section>;
}
