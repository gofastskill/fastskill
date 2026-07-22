// FastSkill browser console (spec 003 phase 3).
//
// Status-first load: /status is fetched before any write control is rendered,
// so the UI never flashes an enabled control that capability gating then
// disables. Read set (project/skills/status) is always available; write set
// (install/update/remove/reindex) is gated on `writable` and, for reindex,
// on `embeddingProvider` too (ADR-0002/0003, spec 003 §6).
class FastSkillApp {
    constructor() {
        this.apiBase = '/api/v1';
        this.status = null;
        this.project = null;
        this.skills = [];
        // id -> { id, outcome, reason, resolvedVersion } from the last
        // `POST /skills/update { check: true }` preflight pass.
        this.preflight = {};
        this.init();
    }

    async init() {
        await this.loadStatus();
        this.applyCapabilities();
        this.setupEventListeners();
        await this.loadAll();
    }

    // ---- status / capability gating -----------------------------------

    async loadStatus() {
        try {
            const res = await fetch(`${this.apiBase}/status`);
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && data.data) {
                this.status = data.data;
            } else {
                throw new Error((data && data.error && data.error.message) || `HTTP ${res.status}`);
            }
        } catch (err) {
            console.error('Failed to load /status:', err);
            // Fail closed: treat capabilities as unavailable rather than
            // risk flashing write controls the server would 403.
            this.status = { writable: false, embeddingProvider: false };
        }
    }

    applyCapabilities() {
        const writable = !!(this.status && this.status.writable);
        const hasProvider = !!(this.status && this.status.embeddingProvider);

        this.toggle('readonly-banner', !writable);
        this.toggle('provider-banner', !hasProvider);
        this.toggle('install-form', writable);
        this.toggle('update-all-btn', writable);
        this.toggle('reindex-btn', writable && hasProvider);
    }

    toggle(id, visible) {
        const el = document.getElementById(id);
        if (!el) return;
        el.hidden = !visible;
    }

    // ---- wiring ---------------------------------------------------------

    setupEventListeners() {
        const searchInput = document.getElementById('search-input');
        if (searchInput) searchInput.addEventListener('input', () => this.renderSkillsTable());

        const installForm = document.getElementById('install-form');
        if (installForm) installForm.addEventListener('submit', (e) => this.handleInstallSubmit(e));

        const updateAllBtn = document.getElementById('update-all-btn');
        if (updateAllBtn) updateAllBtn.addEventListener('click', () => this.updateAll());

        const reindexBtn = document.getElementById('reindex-btn');
        if (reindexBtn) reindexBtn.addEventListener('click', () => this.reindex());

        const drawerClose = document.getElementById('drawer-close');
        if (drawerClose) drawerClose.addEventListener('click', () => this.closeDrawer());

        const drawerBackdrop = document.getElementById('drawer-backdrop');
        if (drawerBackdrop) drawerBackdrop.addEventListener('click', () => this.closeDrawer());

        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape') this.closeDrawer();
        });
    }

    // ---- data loading -----------------------------------------------------

    async loadAll() {
        const loading = document.getElementById('loading');
        const error = document.getElementById('error');
        const content = document.getElementById('content');

        if (loading) loading.hidden = false;
        if (error) { error.hidden = true; error.textContent = ''; }
        if (content) content.hidden = true;

        try {
            const tasks = [
                fetch(`${this.apiBase}/project`),
                fetch(`${this.apiBase}/skills`),
            ];
            const [projectRes, skillsRes] = await Promise.all(tasks);

            if (!projectRes.ok) throw new Error(`Project: HTTP ${projectRes.status}`);
            const projectData = await projectRes.json();
            if (!projectData.success || !projectData.data) {
                throw new Error((projectData.error && projectData.error.message) || 'Failed to load project');
            }
            this.project = projectData.data;

            if (!skillsRes.ok) throw new Error(`Skills: HTTP ${skillsRes.status}`);
            const skillsData = await skillsRes.json();
            this.skills = (skillsData.success && skillsData.data && skillsData.data.skills) || [];

            await this.loadPreflight();

            if (loading) loading.hidden = true;
            if (content) content.hidden = false;
            this.render();
        } catch (err) {
            if (loading) loading.hidden = true;
            if (error) {
                error.hidden = false;
                error.textContent = `Error: ${err.message}`;
            }
            if (content) content.hidden = false;
            this.project = this.project || { metadata: null, skills_directory: '—', skills: [] };
            this.skills = this.skills || [];
            this.render();
            console.error('Load failed:', err);
        }
    }

    // Preflight-classify every installed skill via `check: true` so rows can
    // show an Up to date / Updatable / Pinned badge and only enable a
    // per-row Update button on Updatable skills (spec 003 §4/Q5). Skipped
    // entirely when read-only, since the endpoint is write-gated.
    async loadPreflight() {
        this.preflight = {};
        if (!this.status || !this.status.writable) return;
        try {
            const res = await fetch(`${this.apiBase}/skills/update`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ check: true }),
            });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && Array.isArray(data.data)) {
                for (const result of data.data) {
                    this.preflight[result.id] = result;
                }
            }
        } catch (err) {
            console.error('Preflight check failed:', err);
        }
    }

    // ---- rendering -------------------------------------------------------

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
        if (!m || (!m.id && !m.name && !m.version)) {
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

    // Maps an `Origin` (internally-tagged: git | local | zip-url | repository,
    // see crate::core::origin::Origin) to its salient "Location" field.
    originLocation(origin) {
        if (!origin) return '';
        switch (origin.type) {
            case 'git':
            case 'zip-url':
                return origin.url || '';
            case 'local':
                return origin.path || '';
            case 'repository':
                return [origin.repo, origin.skill].filter(Boolean).join('/');
            default:
                return '';
        }
    }

    badgeFor(id) {
        const p = this.preflight[id];
        if (!p) return '';
        switch (p.outcome) {
            case 'up_to_date':
                return '<span class="badge badge-muted">Up to date</span>';
            case 'would_update':
                return '<span class="badge badge-updatable">Updatable</span>';
            case 'immutable':
                return `<span class="badge badge-pinned" title="${this.escapeHtml(p.reason || '')}">Pinned</span>`;
            case 'error':
                return `<span class="badge badge-error" title="${this.escapeHtml(p.reason || '')}">Check failed</span>`;
            default:
                return '';
        }
    }

    renderSkillsTable() {
        const tbody = document.getElementById('skills-tbody');
        if (!tbody) return;

        const query = (document.getElementById('search-input') || {}).value || '';
        const q = query.toLowerCase().trim();
        const filtered = q
            ? this.skills.filter((s) => {
                const meta = s.metadata || {};
                const origin = meta.origin || null;
                const haystack = [
                    s.name, s.id, s.description, meta.version,
                    origin && origin.type, origin && this.originLocation(origin),
                ].filter(Boolean).join(' ').toLowerCase();
                return haystack.includes(q);
            })
            : this.skills;

        if (filtered.length === 0) {
            tbody.innerHTML = '<tr><td colspan="8" class="empty">No skills</td></tr>';
            return;
        }

        const writable = !!(this.status && this.status.writable);

        tbody.innerHTML = filtered.map((s) => {
            const meta = s.metadata || {};
            const origin = meta.origin || null;
            const name = this.escapeHtml(s.name || '—');
            const id = this.escapeHtml(s.id || '—');
            const version = this.escapeHtml(meta.version || '—');
            const typ = this.escapeHtml((origin && origin.type) || '—');
            const locRaw = origin ? this.originLocation(origin) : '';
            const loc = locRaw
                ? (locRaw.startsWith('http')
                    ? `<a href="${this.escapeHtml(locRaw)}" target="_blank" rel="noopener noreferrer" onclick="event.stopPropagation()">${this.escapeHtml(locRaw)}</a>`
                    : this.escapeHtml(locRaw))
                : '—';
            const descFull = s.description || '';
            const descTrunc = this.escapeHtml(descFull.slice(0, 80));
            const descCell = descFull.length > 80 ? `${descTrunc}…` : (descTrunc || '—');
            const badge = this.badgeFor(s.id) || '<span class="muted">—</span>';

            const pf = this.preflight[s.id];
            const canUpdate = writable && pf && pf.outcome === 'would_update';
            const updateBtn = canUpdate
                ? `<button type="button" class="btn-row btn-update" data-id="${this.escapeHtml(s.id)}">Update</button>`
                : '';
            const removeBtn = writable
                ? `<button type="button" class="btn-row btn-remove" data-id="${this.escapeHtml(s.id)}">Remove</button>`
                : '';

            return `
                <tr data-row-id="${this.escapeHtml(s.id)}">
                    <td class="name">${name}</td>
                    <td class="id"><code>${id}</code></td>
                    <td class="version">${version}</td>
                    <td class="type">${typ}</td>
                    <td class="location">${loc}</td>
                    <td class="description">${descCell}</td>
                    <td class="status">${badge}</td>
                    <td class="actions">${updateBtn}${removeBtn}</td>
                </tr>
            `;
        }).join('');

        tbody.querySelectorAll('tr[data-row-id]').forEach((row) => {
            row.addEventListener('click', () => this.openDrawerById(row.dataset.rowId));
        });
        tbody.querySelectorAll('.btn-update').forEach((btn) => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                this.updateSkill(btn.dataset.id);
            });
        });
        tbody.querySelectorAll('.btn-remove').forEach((btn) => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                this.removeSkill(btn.dataset.id);
            });
        });
    }

    // ---- install / update / remove / reindex ------------------------------

    async handleInstallSubmit(e) {
        e.preventDefault();
        const input = document.getElementById('install-input');
        const btn = document.getElementById('install-btn');
        const value = (input && input.value || '').trim();
        if (!value) return;

        if (input) input.disabled = true;
        if (btn) { btn.disabled = true; btn.textContent = 'Installing…'; }

        try {
            const res = await fetch(`${this.apiBase}/skills/install`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ origin: value, groups: [] }),
            });
            const data = await res.json().catch(() => null);

            if (res.status === 201 && data && data.success) {
                const id = (data.data && data.data.id) || value;
                this.toast(`Installed ${id}`, 'success');
                if (input) input.value = '';
                await this.loadAll();
            } else if (res.status === 409) {
                this.toast((data && data.error && data.error.message) || 'Skill already installed', 'error');
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to install skills', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || `Install failed (HTTP ${res.status})`, 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        } finally {
            if (input) input.disabled = false;
            if (btn) { btn.disabled = false; btn.textContent = 'Install'; }
        }
    }

    summarizeUpdateResults(results) {
        const counts = {};
        for (const r of results) counts[r.outcome] = (counts[r.outcome] || 0) + 1;
        const parts = [];
        if (counts.updated) parts.push(`${counts.updated} updated`);
        if (counts.would_update) parts.push(`${counts.would_update} updatable`);
        if (counts.up_to_date) parts.push(`${counts.up_to_date} up to date`);
        if (counts.immutable) parts.push(`${counts.immutable} pinned`);
        if (counts.error) parts.push(`${counts.error} failed`);
        return parts.length ? parts.join(', ') : 'No skills to update';
    }

    async updateAll() {
        const btn = document.getElementById('update-all-btn');
        if (btn) { btn.disabled = true; btn.textContent = 'Updating…'; }
        try {
            const res = await fetch(`${this.apiBase}/skills/update`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && Array.isArray(data.data)) {
                this.toast(this.summarizeUpdateResults(data.data), 'success');
                await this.loadAll();
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to update skills', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || 'Update failed', 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        } finally {
            if (btn) { btn.disabled = false; btn.textContent = 'Update all'; }
        }
    }

    async updateSkill(skillId) {
        try {
            const res = await fetch(`${this.apiBase}/skills/update`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ skillId }),
            });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && Array.isArray(data.data)) {
                this.toast(this.summarizeUpdateResults(data.data), 'success');
                await this.loadAll();
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to update skills', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || 'Update failed', 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        }
    }

    async removeSkill(skillId) {
        if (!confirm(`Remove skill "${skillId}"? This deletes it from disk.`)) return;
        try {
            const res = await fetch(`${this.apiBase}/skills/${encodeURIComponent(skillId)}`, { method: 'DELETE' });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success) {
                this.toast(`Removed ${skillId}`, 'success');
                await this.loadAll();
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to remove skills', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || 'Remove failed', 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        }
    }

    async reindex() {
        const btn = document.getElementById('reindex-btn');
        if (btn) { btn.disabled = true; btn.textContent = 'Reindexing…'; }
        try {
            const res = await fetch(`${this.apiBase}/reindex`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && data.data) {
                const { reindexed, count, reason } = data.data;
                if (reindexed) {
                    this.toast(`Reindexed ${count} skill${count === 1 ? '' : 's'}`, 'success');
                } else {
                    this.toast(reason || 'Reindex skipped', 'info');
                }
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to reindex', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || 'Reindex failed', 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        } finally {
            if (btn) { btn.disabled = false; btn.textContent = 'Reindex'; }
        }
    }

    // ---- detail drawer -----------------------------------------------------

    openDrawerById(skillId) {
        const skill = this.skills.find((s) => s.id === skillId);
        if (skill) this.openDrawer(skill);
    }

    async openDrawer(skill) {
        const drawer = document.getElementById('drawer');
        const title = document.getElementById('drawer-title');
        const body = document.getElementById('drawer-body');
        if (!drawer || !title || !body) return;

        title.textContent = skill.name || skill.id;

        const meta = skill.metadata || {};
        const origin = meta.origin || null;
        const originRows = origin
            ? `
                <tr><td class="k">origin type</td><td>${this.escapeHtml(origin.type || '—')}</td></tr>
                <tr><td class="k">origin location</td><td class="break-all">${this.escapeHtml(this.originLocation(origin) || '—')}</td></tr>
              `
            : `<tr><td class="k">origin</td><td class="muted">—</td></tr>`;

        // Version picker (v2): only repository-origin skills carry a version
        // index to pick from — git/local/zip-url are versionless (spec 003
        // §3, ADR-0004 scope) and render nothing here.
        const isRepository = !!(origin && origin.type === 'repository');
        const versionSection = isRepository
            ? `
                <h3 class="drawer-subtitle">Version</h3>
                <div class="version-picker" id="drawer-version-section">
                    <select id="drawer-version-select" class="version-select" hidden disabled></select>
                    <button type="button" id="drawer-version-apply" class="btn btn-secondary" hidden disabled>Apply</button>
                    <p class="muted" id="drawer-version-note">Loading versions&hellip;</p>
                </div>
            `
            : '';

        body.innerHTML = `
            <table class="meta-table"><tbody>
                <tr><td class="k">id</td><td><code>${this.escapeHtml(skill.id)}</code></td></tr>
                <tr><td class="k">name</td><td>${this.escapeHtml(skill.name || '—')}</td></tr>
                <tr><td class="k">version</td><td>${this.escapeHtml(meta.version || '—')}</td></tr>
                <tr><td class="k">author</td><td>${this.escapeHtml(meta.author || '—')}</td></tr>
                ${originRows}
            </tbody></table>
            ${versionSection}
            <h3 class="drawer-subtitle">SKILL.md</h3>
            <div class="content-toggle" role="group" aria-label="Content view">
                <button type="button" class="toggle-btn active" id="drawer-format-raw" data-format="raw">Raw</button>
                <button type="button" class="toggle-btn" id="drawer-format-rendered" data-format="rendered">Rendered</button>
            </div>
            <pre class="skill-content" id="drawer-content">Loading&hellip;</pre>
            <div class="markdown-body" id="drawer-content-rendered" hidden></div>
        `;

        drawer.classList.add('open');
        drawer.setAttribute('aria-hidden', 'false');
        this._openSkillId = skill.id;
        // format -> fetched content, so toggling Raw/Rendered back and forth
        // doesn't refetch on every click.
        this._contentCache = { raw: null, rendered: null };

        const rawBtn = document.getElementById('drawer-format-raw');
        const renderedBtn = document.getElementById('drawer-format-rendered');
        if (rawBtn) rawBtn.addEventListener('click', () => this.setContentFormat(skill.id, 'raw'));
        if (renderedBtn) renderedBtn.addEventListener('click', () => this.setContentFormat(skill.id, 'rendered'));

        if (isRepository) {
            const versionApplyBtn = document.getElementById('drawer-version-apply');
            if (versionApplyBtn) versionApplyBtn.addEventListener('click', () => this.applyVersionChange(skill));
            this.loadVersionPicker(skill);
        }

        // Raw is selected by default (unchanged v1 behavior).
        this.setContentFormat(skill.id, 'raw');
    }

    // Switches the drawer's SKILL.md view between the escaped raw <pre> and
    // the server-rendered/sanitized markdown container, fetching content for
    // that format on first use and reusing the cache afterward.
    setContentFormat(skillId, format) {
        this._contentFormat = format;
        const rawBtn = document.getElementById('drawer-format-raw');
        const renderedBtn = document.getElementById('drawer-format-rendered');
        const contentEl = document.getElementById('drawer-content');
        const renderedEl = document.getElementById('drawer-content-rendered');
        if (rawBtn) rawBtn.classList.toggle('active', format === 'raw');
        if (renderedBtn) renderedBtn.classList.toggle('active', format === 'rendered');
        if (contentEl) contentEl.hidden = format !== 'raw';
        if (renderedEl) renderedEl.hidden = format !== 'rendered';
        this.loadDrawerContent(skillId, format);
    }

    async loadDrawerContent(skillId, format) {
        const cache = this._contentCache || (this._contentCache = { raw: null, rendered: null });
        if (cache[format] != null) {
            this.applyContentToDom(format, cache[format]);
            return;
        }

        const contentEl = document.getElementById('drawer-content');
        const renderedEl = document.getElementById('drawer-content-rendered');
        if (format === 'raw') {
            if (contentEl) contentEl.textContent = 'Loading…';
        } else if (renderedEl) {
            renderedEl.textContent = 'Loading…';
        }

        try {
            const url = format === 'rendered'
                ? `${this.apiBase}/skills/${encodeURIComponent(skillId)}/content?format=html`
                : `${this.apiBase}/skills/${encodeURIComponent(skillId)}/content`;
            const res = await fetch(url);
            const data = await res.json().catch(() => null);
            // The drawer may have been closed/reopened on a different skill
            // while this request was in flight.
            if (this._openSkillId !== skillId) return;

            if (res.ok && data && data.success && data.data) {
                const value = data.data.content != null ? data.data.content : '';
                cache[format] = value;
                this.applyContentToDom(format, value);
            } else {
                const msg = `Error loading content: ${(data && data.error && data.error.message) || res.status}`;
                if (format === 'raw') { if (contentEl) contentEl.textContent = msg; }
                else if (renderedEl) renderedEl.textContent = msg;
            }
        } catch (err) {
            if (this._openSkillId !== skillId) return;
            const msg = `Error: ${err.message}`;
            if (format === 'raw') { if (contentEl) contentEl.textContent = msg; }
            else if (renderedEl) renderedEl.textContent = msg;
        }
    }

    applyContentToDom(format, value) {
        const contentEl = document.getElementById('drawer-content');
        const renderedEl = document.getElementById('drawer-content-rendered');
        if (format === 'raw') {
            if (!contentEl) return;
            // textContent, not innerHTML: the SKILL.md raw body is untrusted
            // content (SEC-7) and must never be interpreted as markup.
            contentEl.textContent = value;
            return;
        }
        if (!renderedEl) return;
        // INTENTIONAL innerHTML, NOT a SEC-7 violation: `value` here is the
        // *rendered* markdown response (`?format=html`), which the Rust
        // server already ran through comrak + an ammonia allowlist
        // sanitizer before sending it — it is not the raw untrusted
        // SKILL.md text. Re-escaping it here would print literal `<h1>`
        // tags as text instead of rendering them. Do NOT change this to
        // textContent/escapeHtml; if you need to distrust this value, fix
        // the sanitization server-side instead.
        renderedEl.innerHTML = value;
        // Extra client-side safety net on top of server sanitization: force
        // every link to open in a new tab without leaking a window.opener
        // handle back to the linked page. Operates on the already-sanitized
        // DOM (not on raw untrusted text), so this is safe regardless of
        // what the server did or didn't set.
        renderedEl.querySelectorAll('a[href]').forEach((a) => {
            a.setAttribute('target', '_blank');
            a.setAttribute('rel', 'noopener noreferrer');
        });
    }

    // ---- version picker (repository-origin skills only) -------------------

    async loadVersionPicker(skill) {
        const section = document.getElementById('drawer-version-section');
        if (!section) return;

        const select = document.getElementById('drawer-version-select');
        const applyBtn = document.getElementById('drawer-version-apply');
        const note = document.getElementById('drawer-version-note');
        const writable = !!(this.status && this.status.writable);

        try {
            const res = await fetch(`${this.apiBase}/registry/skills/${encodeURIComponent(skill.id)}/versions`);
            const data = await res.json().catch(() => null);
            if (this._openSkillId !== skill.id) return;

            const versions = (res.ok && data && data.success && data.data && Array.isArray(data.data.versions))
                ? data.data.versions
                : [];

            if (versions.length === 0) {
                if (select) select.hidden = true;
                if (applyBtn) applyBtn.hidden = true;
                if (note) {
                    note.hidden = false;
                    note.textContent = 'No other versions available';
                }
                return;
            }

            const meta = skill.metadata || {};
            const origin = meta.origin || {};
            const current = origin.version || meta.version || null;

            if (select) {
                select.innerHTML = versions.map((v) => {
                    const val = this.escapeHtml(v.version);
                    const selected = current && v.version === current ? ' selected' : '';
                    return `<option value="${val}"${selected}>${val}</option>`;
                }).join('');
                select.disabled = !writable;
                select.hidden = false;
            }
            if (applyBtn) {
                applyBtn.disabled = !writable;
                applyBtn.hidden = false;
            }
            if (note) note.hidden = true;
        } catch (err) {
            if (this._openSkillId !== skill.id) return;
            if (select) select.hidden = true;
            if (applyBtn) applyBtn.hidden = true;
            if (note) {
                note.hidden = false;
                note.textContent = `Error loading versions: ${err.message}`;
            }
        }
    }

    async applyVersionChange(skill) {
        if (!(this.status && this.status.writable)) {
            this.toast('Read-only — start with --enable-write to update skills', 'error');
            return;
        }

        const select = document.getElementById('drawer-version-select');
        const applyBtn = document.getElementById('drawer-version-apply');
        if (!select || !select.value) return;

        if (applyBtn) { applyBtn.disabled = true; applyBtn.textContent = 'Applying…'; }
        try {
            const res = await fetch(`${this.apiBase}/skills/update`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ skillId: skill.id, version: select.value }),
            });
            const data = await res.json().catch(() => null);
            if (res.ok && data && data.success && Array.isArray(data.data)) {
                this.toast(this.summarizeUpdateResults(data.data), 'success');
                await this.loadAll();
                // Refresh the drawer in place so metadata/version reflect
                // the just-applied change, if the skill is still installed.
                const refreshed = this.skills.find((s) => s.id === skill.id);
                if (refreshed) await this.openDrawer(refreshed);
            } else if (res.status === 403) {
                this.toast('Read-only — start with --enable-write to update skills', 'error');
            } else {
                this.toast((data && data.error && data.error.message) || 'Version update failed', 'error');
            }
        } catch (err) {
            this.toast(`Error: ${err.message}`, 'error');
        } finally {
            // No-op if openDrawer already replaced the drawer body above.
            const btn = document.getElementById('drawer-version-apply');
            if (btn) { btn.disabled = !(this.status && this.status.writable); btn.textContent = 'Apply'; }
        }
    }

    closeDrawer() {
        const drawer = document.getElementById('drawer');
        if (!drawer) return;
        drawer.classList.remove('open');
        drawer.setAttribute('aria-hidden', 'true');
        this._openSkillId = null;
        this._contentCache = null;
    }

    // ---- toasts -----------------------------------------------------------

    toast(message, kind) {
        const container = document.getElementById('toast-container');
        if (!container) return;

        const el = document.createElement('div');
        el.className = `toast toast-${kind || 'info'}`;
        el.textContent = message;
        container.appendChild(el);

        // Force layout before adding .show so the transition runs.
        requestAnimationFrame(() => el.classList.add('show'));

        setTimeout(() => {
            el.classList.remove('show');
            setTimeout(() => el.remove(), 250);
        }, 3200);
    }

    // ---- utilities ----------------------------------------------------------

    escapeHtml(text) {
        // Escapes for BOTH element-text and quoted-attribute contexts. The
        // textContent/innerHTML trick only escapes & < >, leaving " and '
        // intact — which is an attribute-breakout XSS when interpolated into
        // title="…" / href="…" / data-id="…" with untrusted skill content
        // (SEC-7/SEC-8). Escape all five explicitly; & must go first.
        if (text == null) return '';
        return String(text)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }
}

const app = new FastSkillApp();
