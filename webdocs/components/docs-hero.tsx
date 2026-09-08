import Link from 'next/link';

export function FastSkillHero() {
  return (
    <section className="docs-hero" aria-labelledby="docs-hero-title">
      <div className="docs-hero-copy">
        <p className="docs-eyebrow">
          <span aria-hidden="true" />
          Package operations for agent skills
        </p>
        <h2 id="docs-hero-title">Ship better skills. Keep every agent in sync.</h2>
        <p>
          Install, lock, bundle, discover, and evaluate the skills your agents already
          understand.
        </p>
        <div className="docs-hero-actions">
          <Link href="/quickstart" className="docs-primary-action">
            Start in five minutes <span aria-hidden="true">→</span>
          </Link>
          <a
            href="https://github.com/gofastskill/fastskill"
            className="docs-secondary-action"
          >
            View source <span aria-hidden="true">↗</span>
          </a>
        </div>
        <ul className="docs-badges" aria-label="FastSkill project attributes">
          <li>Apache-2.0</li>
          <li>Built with Rust</li>
          <li>MCP ready</li>
        </ul>
      </div>
      <div className="docs-terminal" aria-label="FastSkill command example">
        <div className="docs-terminal-bar">
          <span className="docs-terminal-dots" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          team-skills / terminal
        </div>
        <div className="docs-terminal-body">
          <p><b>$</b> fastskill init</p>
          <span>✓ Created skill-project.toml</span>
          <p><b>$</b> fastskill install</p>
          <span>✓ Locked 8 skills for the team</span>
          <p><b>$</b> fastskill bundle build --output dist</p>
          <span className="docs-terminal-bright">✓ Built team-stack-1.0.0.zip</span>
        </div>
      </div>
    </section>
  );
}
