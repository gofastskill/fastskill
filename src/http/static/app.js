// FastSkill UI - skill-project.toml view; table lists all installed skills
class ProjectApp {
    constructor() {
        this.apiBase = '/api';
        this.project = null;
        this.skills = [];
        this.init();
    }

    async init() {
        this.setupEventListeners();
        await this.loadAll();
    }

    setupEventListeners() {
        const searchInput = document.getElementById('search-input');
        if (searchInput) {
            searchInput.addEventListener('input', () => this.render());
        }
        const upgradeAllBtn = document.getElementById('upgrade-all-btn');
        if (upgradeAllBtn) {
            upgradeAllBtn.addEventListener('click', () => this.upgradeAll());
        }
    }

    async loadAll() {
        const loading = document.getElementById('loading');
        const error = document.getElementById('error');
        const content = document.getElementById('content');

        if (loading) loading.style.display = 'block';
        if (error) { error.style.display = 'none'; error.textContent = ''; }
        if (content) content.style.display = 'none';

        try {
            const [projectRes, skillsRes] = await Promise.all([
                fetch(`${this.apiBase}/project`),
                fetch(`${this.apiBase}/skills`),
            ]);

            if (!projectRes.ok) throw new Error(`Project: HTTP ${projectRes.status}`);
            const projectData = await projectRes.json();
            if (!projectData.success || !projectData.data) {
                throw new Error(projectData.error?.message || 'Failed to load project');
            }
            this.project = projectData.data;

            if (!skillsRes.ok) throw new Error(`Skills: HTTP ${skillsRes.status}`);
            const skillsData = await skillsRes.json();
            if (skillsData.success && skillsData.data && skillsData.data.skills) {
                this.skills = skillsData.data.skills;
            } else {
                this.skills = [];
            }

            if (loading) loading.style.display = 'none';
            if (content) content.style.display = 'block';
            this.render();
        } catch (err) {
            if (loading) loading.style.display = 'none';
            if (error) {
                error.style.display = 'block';
                error.textContent = `Error: ${err.message}`;
            }
            if (content) content.style.display = 'block';
            this.project = this.project || { metadata: null, skills_directory: '—', skills: [] };
            this.skills = this.skills || [];
            this.render();
            console.error('Load failed:', err);
        }
    }

    render() {
        if (!this.project) return;

        this.renderMetadata();
        this.renderSkillsDirectory();
        this.renderSkillsTable();
    }

    renderMetadata() {
        const el = document.getElementById('metadata-section');
        if (!el) return;

        const m = this.project.metadata;
        if (!m || (m && !m.id && !m.name && !m.version)) {
            el.innerHTML = '<h2 class="section-title">[metadata]</h2><p class="muted">No metadata</p>';
            return;
        }

        const rows = [];
        if (m.id) rows.push(`<tr><td class="k">id</td><td>${this.escapeHtml(m.id)}</td></tr>`);
        if (m.name) rows.push(`<tr><td class="k">name</td><td>${this.escapeHtml(m.name)}</td></tr>`);
        if (m.version) rows.push(`<tr><td class="k">version</td><td>${this.escapeHtml(m.version)}</td></tr>`);
        if (m.description) rows.push(`<tr><td class="k">description</td><td>${this.escapeHtml(m.description)}</td></tr>`);
        if (m.author) rows.push(`<tr><td class="k">author</td><td>${this.escapeHtml(m.author)}</td></tr>`);

        el.innerHTML = `
            <h2 class="section-title">[metadata]</h2>
            <table class="meta-table"><tbody>${rows.join('')}</tbody></table>
        `;
    }

    renderSkillsDirectory() {
        const el = document.getElementById('skills-dir-section');
        if (!el) return;

        const dir = this.project.skills_directory || '—';
        el.innerHTML = `
            <h2 class="section-title">[tool.fastskill]</h2>
            <table class="meta-table"><tbody>
                <tr><td class="k">skills_directory</td><td><code>${this.escapeHtml(dir)}</code></td></tr>
            </tbody></table>
        `;
    }

    renderSkillsTable() {
        const tbody = document.getElementById('skills-tbody');
        if (!tbody) return;

        const skills = this.skills;
        const query = (document.getElementById('search-input') || {}).value || '';
        const q = query.toLowerCase().trim();
        const filtered = q
            ? skills.filter(s => {
                const name = (s.name || '').toLowerCase();
                const id = (s.id || '').toLowerCase();
                const desc = (s.description || '').toLowerCase();
                const meta = s.metadata || {};
                const ver = (meta.version || '').toLowerCase();
                const loc = (meta.source_url || '').toLowerCase();
                const typ = (meta.source_type || '').toLowerCase();
                return name.includes(q) || id.includes(q) || desc.includes(q) || ver.includes(q) || loc.includes(q) || typ.includes(q);
            })
            : skills;

        if (filtered.length === 0) {
            tbody.innerHTML = '<tr><td colspan="7" class="empty">No skills</td></tr>';
            return;
        }

        tbody.innerHTML = filtered.map(s => {
            const meta = s.metadata || {};
            const name = this.escapeHtml(s.name || '—');
            const id = this.escapeHtml(s.id || '—');
            const version = this.escapeHtml(meta.version || '—');
            const typ = this.escapeHtml(String(meta.source_type || '—'));
            const locRaw = meta.source_url || '';
            const loc = locRaw
                ? (locRaw.startsWith('http') ? `<a href="${this.escapeHtml(locRaw)}" target="_blank" rel="noopener">${this.escapeHtml(locRaw)}</a>` : this.escapeHtml(locRaw))
                : '—';
            const desc = this.escapeHtml((s.description || '').slice(0, 80));
            const descCell = (s.description || '').length > 80 ? desc + '…' : desc || '—';
            return `
                <tr>
                    <td class="name">${name}</td>
                    <td class="id"><code>${id}</code></td>
                    <td class="version">${version}</td>
                    <td class="type">${typ}</td>
                    <td class="location">${loc}</td>
                    <td class="description">${descCell}</td>
                    <td class="actions">
                        <button type="button" class="btn-upgrade" data-id="${this.escapeHtml(s.id)}">Upgrade</button>
                        <button type="button" class="btn-remove" data-id="${this.escapeHtml(s.id)}">Remove</button>
                    </td>
                </tr>
            `;
        }).join('');

        tbody.querySelectorAll('.btn-upgrade').forEach(btn => {
            btn.addEventListener('click', () => this.upgradeSkill(btn.dataset.id));
        });
        tbody.querySelectorAll('.btn-remove').forEach(btn => {
            btn.addEventListener('click', () => this.removeSkill(btn.dataset.id));
        });
    }

    async upgradeAll() {
        const btn = document.getElementById('upgrade-all-btn');
        if (btn) { btn.disabled = true; btn.textContent = 'Upgrading...'; }
        try {
            const response = await fetch(`${this.apiBase}/skills/upgrade`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            });
            const data = await response.json();
            if (data.success) {
                await this.loadAll();
                alert(data.data?.message || 'Upgrade completed');
            } else {
                alert(data.error?.message || 'Upgrade failed');
            }
        } catch (err) {
            alert(`Error: ${err.message}`);
        } finally {
            if (btn) { btn.disabled = false; btn.textContent = 'Upgrade all'; }
        }
    }

    async upgradeSkill(skillId) {
        try {
            const response = await fetch(`${this.apiBase}/skills/upgrade`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ skillId: skillId }),
            });
            const data = await response.json();
            if (data.success) await this.loadAll();
            else alert(data.error?.message || 'Upgrade failed');
        } catch (err) {
            alert(`Error: ${err.message}`);
        }
    }

    async removeSkill(skillId) {
        if (!confirm(`Remove skill ${skillId}?`)) return;
        try {
            const response = await fetch(`${this.apiBase}/skills/${encodeURIComponent(skillId)}`, { method: 'DELETE' });
            const data = await response.json();
            if (data.success) await this.loadAll();
            else alert(data.error?.message || 'Remove failed');
        } catch (err) {
            alert(`Error: ${err.message}`);
        }
    }

    escapeHtml(text) {
        if (text == null) return '';
        const div = document.createElement('div');
        div.textContent = String(text);
        return div.innerHTML;
    }
}

const app = new ProjectApp();
