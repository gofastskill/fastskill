export function FastSkillMark({ className = '' }: { className?: string }) {
  return (
    <span className={`fastskill-mark ${className}`} aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M12 2v6m0 8v6M2 12h6m8 0h6M5 5l4 4m6 6 4 4m0-14-4 4m-6 6-4 4" />
      </svg>
    </span>
  );
}

export function FastSkillBrand() {
  return (
    <span className="fastskill-brand">
      <FastSkillMark />
      <span>FastSkill</span>
    </span>
  );
}
