/** The languages Keel speaks (ADR-0038): a register, a kitchen ticket, a member. */
export const LANGUAGES: [string, string][] = [
  ["en-US", "English"],
  ["es-US", "Español"],
];

/** A language, or the register's when none is chosen. */
export function LanguageSelect({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string | undefined;
  onChange: (language: string | undefined) => void;
}) {
  return (
    <select aria-label={label} value={value ?? ""} onChange={(event) => onChange(event.target.value || undefined)}>
      <option value="">The register's</option>
      {LANGUAGES.map(([tag, words]) => (
        <option key={tag} value={tag}>
          {words}
        </option>
      ))}
    </select>
  );
}
