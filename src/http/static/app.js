// FastSkill Registry UI
class RegistryApp {
    constructor() {
        this.apiBase = '/api';
        this.allSkills = [];
        this.filteredSkills = [];
        this.manifestSkills = new Set(); // Set of skill IDs in manifest
        this.installedSkills = new Set(); // Set of skill IDs that are installed
        this.currentSource = null;
        this.virtualScroll = {
            container: null,
            itemHeight: 200, // Approximate height of skill card
            visibleItems: 0,
            startIndex: 0,
            endIndex: 0,
        };
        this.init();
    }

    async init() {
        this.setupEventListeners();
        await this.loadSources();
        await this.loadManifestSkills();
        await this.loadSkills();
        this.setupVirtualScroll();
    }

    setupEventListeners() {
        document.getElementById('search-input').addEventListener('input', (e) => {
            this.handleSearch(e.target.value);
        });
        document.getElementById('search-btn').addEventListener('click', () => {
            const query = document.getElementById('search-input').value;
            this.handleSearch(query);
        });
        document.getElementById('source-filter').addEventListener('change', (e) => {
            this.filterBySource(e.target.value);
        });
        document.getElementById('status-filter').addEventListener('change', (e) => {
            this.filterByStatus(e.target.value);
        });
        const refreshBtn = document.getElementById('refresh-btn');
        if (refreshBtn) {
            refreshBtn.addEventListener('click', () => {
                this.refreshSources();
            });
        }
    }

    async loadSources() {
        try {
            const response = await fetch(`${this.apiBase}/registry/sources`);
            const data = await response.json();
            if (data.success && data.data) {
                this.renderSources(data.data);
            }
        } catch (error) {
            console.error('Failed to load sources:', error);
        }
    }

    async loadManifestSkills() {
        try {
            const response = await fetch(`${this.apiBase}/manifest/skills`);
            const data = await response.json();
            if (data.success && data.data) {
                this.manifestSkills = new Set(data.data.map(skill => skill.id));
            }
        } catch (error) {
            console.error('Failed to load manifest skills:', error);
            // Continue even if manifest load fails
        }
    }

    async loadSkills() {
        const loading = document.getElementById('loading');
        const error = document.getElementById('error');
        const container = document.getElementById('skills-container');
        
        loading.style.display = 'block';
        error.style.display = 'none';
        container.style.display = 'none';

        try {
            const response = await fetch(`${this.apiBase}/registry/skills`);
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            const data = await response.json();
            console.log('Skills API response:', data);
            
            if (data.success && data.data) {
                this.allSkills = [];
                this.installedSkills = new Set();
                
                // Handle empty sources array
                if (data.data.sources && Array.isArray(data.data.sources)) {
                    data.data.sources.forEach(source => {
                        if (source.skills && Array.isArray(source.skills)) {
                            source.skills.forEach(skill => {
                                // Mark installed skills
                                if (skill.installed) {
                                    this.installedSkills.add(skill.id);
                                }
                                // Add status information
                                skill.inManifest = this.manifestSkills.has(skill.id);
                                this.allSkills.push(skill);
                            });
                        }
                    });
                }
                
                this.filteredSkills = [...this.allSkills];
                loading.style.display = 'none';
                
                if (this.allSkills.length === 0) {
                    // Show message when no skills are available
                    container.innerHTML = '<div class="no-skills">No skills found. Make sure your sources have valid marketplace.json files.</div>';
                    container.style.display = 'block';
                } else {
                    container.style.display = 'block';
                    this.renderSkills();
                }
            } else {
                throw new Error(data.error?.message || data.error || 'Failed to load skills');
            }
        } catch (err) {
            loading.style.display = 'none';
            error.style.display = 'block';
            error.textContent = `Error: ${err.message}`;
            console.error('Failed to load skills:', err);
            console.error('Full error details:', err);
        }
    }

    renderSources(sources) {
        const sourcesList = document.getElementById('sources-list');
        const sourceFilter = document.getElementById('source-filter');
        
        sourcesList.innerHTML = '';
        sourceFilter.innerHTML = '<option value="">All Sources</option>';
        
        sources.forEach(source => {
            // Sidebar list
            const li = document.createElement('li');
            li.textContent = source.name;
            li.dataset.source = source.name;
            li.addEventListener('click', () => {
                document.querySelectorAll('#sources-list li').forEach(item => {
                    item.classList.remove('active');
                });
                li.classList.add('active');
                this.filterBySource(source.name);
            });
            sourcesList.appendChild(li);
            
            // Filter dropdown
            const option = document.createElement('option');
            option.value = source.name;
            option.textContent = source.name;
            sourceFilter.appendChild(option);
        });
    }

    renderSkills() {
        const container = document.getElementById('skills-list');
        container.innerHTML = '';
        
        if (this.filteredSkills.length === 0) {
            container.innerHTML = '<div class="no-results">No skills found</div>';
            return;
        }

        // Simple virtual scrolling: render visible items + buffer
        const viewportHeight = container.parentElement.clientHeight;
        const visibleCount = Math.ceil(viewportHeight / this.virtualScroll.itemHeight) + 2;
        
        const start = Math.max(0, this.virtualScroll.startIndex);
        const end = Math.min(this.filteredSkills.length, start + visibleCount);
        
        // Render visible items
        for (let i = start; i < end; i++) {
            const skill = this.filteredSkills[i];
            const card = this.createSkillCard(skill);
            container.appendChild(card);
        }
        
        // Update scroll handler
        this.updateVirtualScroll();
    }

    createSkillCard(skill) {
        const card = document.createElement('div');
        card.className = `skill-card ${skill.installed ? 'installed' : 'available'}`;
        
        const tagsHtml = skill.tags.map(tag => `<span class="tag">${this.escapeHtml(tag)}</span>`).join('');
        const capabilitiesHtml = skill.capabilities.map(cap => `<span class="tag">${this.escapeHtml(cap)}</span>`).join('');
        
        // Build status badges
        const statusBadges = [];
        if (skill.inManifest) {
            statusBadges.push('<span class="status-badge in-manifest">In Manifest</span>');
        }
        if (skill.installed) {
            statusBadges.push('<span class="status-badge installed">Installed</span>');
        }
        if (!skill.inManifest && !skill.installed) {
            statusBadges.push('<span class="status-badge available">Available</span>');
        }
        
        card.innerHTML = `
            <div class="skill-header">
                <div>
                    <div class="skill-name">${this.escapeHtml(skill.name)}</div>
                    <div class="skill-meta">
                        <span>Version: ${this.escapeHtml(skill.version)}</span>
                        ${skill.author ? `<span>Author: ${this.escapeHtml(skill.author)}</span>` : ''}
                        <span>Source: ${this.escapeHtml(skill.sourceName)}</span>
                    </div>
                </div>
                <div class="skill-status-badges">
                    ${statusBadges.join('')}
                </div>
            </div>
            <div class="skill-description">${this.escapeHtml(skill.description)}</div>
            ${tagsHtml ? `<div class="skill-tags">${tagsHtml}</div>` : ''}
            ${capabilitiesHtml ? `<div class="skill-tags">Capabilities: ${capabilitiesHtml}</div>` : ''}
            <div class="skill-actions">
                ${skill.inManifest 
                    ? `<button class="btn-uninstall" onclick="app.uninstallSkill('${this.escapeHtml(skill.id)}')">Remove from Manifest</button>`
                    : `<button class="btn-install" onclick="app.installSkill('${this.escapeHtml(skill.id)}', '${this.escapeHtml(skill.sourceName)}')">Add to Manifest</button>`
                }
            </div>
        `;
        
        return card;
    }

    setupVirtualScroll() {
        const container = document.getElementById('skills-container');
        container.addEventListener('scroll', () => {
            this.updateVirtualScroll();
        });
    }

    updateVirtualScroll() {
        const container = document.getElementById('skills-container');
        const scrollTop = container.scrollTop;
        const startIndex = Math.floor(scrollTop / this.virtualScroll.itemHeight);
        
        if (startIndex !== this.virtualScroll.startIndex) {
            this.virtualScroll.startIndex = startIndex;
            this.renderSkills();
        }
    }

    handleSearch(query) {
        const lowerQuery = query.toLowerCase().trim();
        if (!lowerQuery) {
            this.filteredSkills = [...this.allSkills];
        } else {
            this.filteredSkills = this.allSkills.filter(skill => {
                return skill.name.toLowerCase().includes(lowerQuery) ||
                       skill.description.toLowerCase().includes(lowerQuery) ||
                       skill.tags.some(tag => tag.toLowerCase().includes(lowerQuery)) ||
                       skill.capabilities.some(cap => cap.toLowerCase().includes(lowerQuery));
            });
        }
        this.renderSkills();
    }

    filterBySource(sourceName) {
        if (!sourceName) {
            this.filteredSkills = [...this.allSkills];
        } else {
            this.filteredSkills = this.allSkills.filter(skill => skill.sourceName === sourceName);
        }
        this.renderSkills();
    }

    filterByStatus(status) {
        if (!status) {
            this.filteredSkills = [...this.allSkills];
        } else {
            this.filteredSkills = this.allSkills.filter(skill => {
                return status === 'installed' ? skill.installed : !skill.installed;
            });
        }
        this.renderSkills();
    }

    async installSkill(skillId, sourceName) {
        try {
            const response = await fetch(`${this.apiBase}/manifest/skills`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    skillId: skillId,
                    sourceName: sourceName,
                }),
            });
            
            const data = await response.json();
            if (data.success) {
                // Reload manifest skills and all skills to update status
                await this.loadManifestSkills();
                await this.loadSkills();
            } else {
                alert(`Failed to add skill: ${data.error?.message || 'Unknown error'}`);
            }
        } catch (error) {
            alert(`Error adding skill: ${error.message}`);
            console.error('Failed to add skill:', error);
        }
    }

    async uninstallSkill(skillId) {
        if (!confirm(`Remove skill ${skillId} from manifest?`)) {
            return;
        }
        
        try {
            const response = await fetch(`${this.apiBase}/manifest/skills/${encodeURIComponent(skillId)}`, {
                method: 'DELETE',
            });
            
            const data = await response.json();
            if (data.success) {
                // Reload manifest skills and all skills to update status
                await this.loadManifestSkills();
                await this.loadSkills();
            } else {
                alert(`Failed to remove skill: ${data.error?.message || 'Unknown error'}`);
            }
        } catch (error) {
            alert(`Error removing skill: ${error.message}`);
            console.error('Failed to remove skill:', error);
        }
    }

    async refreshSources() {
        const refreshBtn = document.getElementById('refresh-btn');
        if (refreshBtn) {
            refreshBtn.disabled = true;
            refreshBtn.textContent = 'Refreshing...';
        }
        
        try {
            const response = await fetch(`${this.apiBase}/registry/refresh`, {
                method: 'POST',
            });
            
            const data = await response.json();
            if (data.success) {
                // Reload skills after refresh
                await this.loadSkills();
            } else {
                alert(`Failed to refresh: ${data.error?.message || 'Unknown error'}`);
            }
        } catch (error) {
            alert(`Error refreshing: ${error.message}`);
            console.error('Failed to refresh sources:', error);
        } finally {
            if (refreshBtn) {
                refreshBtn.disabled = false;
                refreshBtn.textContent = 'Refresh';
            }
        }
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

// Initialize app
const app = new RegistryApp();

