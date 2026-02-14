---
title: CodeScene Health Improvement Plan for fastskill
created: 2026-02-13
status: draft
project: fastskill
target_score: >8.0 for all files
---

# CodeScene Health Improvement Plan

## Executive Summary

This plan addresses code health issues identified by CodeScene in the fastskill codebase. The analysis of 117 Rust source files revealed 15 files with code health issues below the target score of 8.0. This plan provides precise problem identification and actionable recommendations to achieve target scores.

## Current Score Distribution

### Critical Files (Score < 8.0) - Immediate Action Required

| File | Current Score | Primary Issues |
|------|---------------|----------------|
| `cli/commands/remove.rs` | 7.99 | Extreme complexity (cc=35), 6-level nesting, 7 bumpy road sections |
| `cli/commands/publish.rs` | 7.39 | Complex method (cc=12), 4-level nesting, 5 function arguments, code duplication |
| `http/server.rs` | 7.93 | Complex method (cc=15), 2 bumpy road sections, code duplication, 101-line function |

### Moderate Concern Files (Score 8.0 - 8.6) - High Priority

| File | Current Score | Primary Issues |
|------|---------------|----------------|
| `cli/commands/install.rs` | 8.13 | Extreme complexity (cc=19), 4-level nesting, 130-line function |
| `cli/commands/list.rs` | 8.6 | Complex method (cc=21), 98-line function, bumpy road |
| `validation/standard_validator.rs` | 8.54 | 4-level nesting, bumpy road, code duplication |

### Low Concern Files (Score 8.6 - 9.1) - Medium Priority

| File | Current Score | Primary Issues |
|------|---------------|----------------|
| `storage/git.rs` | 9.09 | Complex method (cc=9), complex conditionals |
| `cli/commands/sync.rs` | 9.54 | 81-line function |
| `validation/skill_validator.rs` | 9.38 | Code duplication |
| `storage/filesystem.rs` | 9.38 | 4-level nesting |
| `core/service.rs` | 9.38 | Complex method (cc=9) |
| `execution.rs` | 9.84 | Bumpy road |

### Healthy Files (Score >= 9.8)

| File | Score |
|------|-------|
| `cli/commands/init.rs` | 10.0 |
| `cli/commands/serve.rs` | 10.0 |
| `core/repository.rs` | 10.0 |
| `cli/commands/add.rs` | 10.0 |

## Detailed File Analysis and Recommendations

---

## 1. cli/commands/remove.rs (Score: 7.99)

### Problems Identified

**Critical Issues:**

1. **Extreme Cyclomatic Complexity** (Severity: Critical)
   - Function: `execute_remove` (lines 28-210)
   - Complexity: cc=35 (recommended: <9)
   - Impact: Brain Method - Complex Method - Long Method

2. **Deep Nested Complexity** (Severity: Critical)
   - Function: `execute_remove` (lines 28-210)
   - Nesting depth: 6 conditionals (recommended: <4)
   - Correlation: ~20% of programming mistakes attributed to deep nesting

3. **Bumpy Road Ahead** (Severity: Critical)
   - Function: `execute_remove` (lines 28-210)
   - Bumps: 7 (extremely high)
   - Impact: Lack of encapsulation, feature entanglement, complex state management

4. **Large Method** (Severity: High)
   - Function: `execute_remove` (lines 28-210)
   - Lines of code: 152 (recommended: <70)

### Recommended Refactoring Actions

1. **Extract Command Pattern**
   ```rust
   // Create a trait for remove operations
   trait RemoveOperation {
       fn can_remove(&self) -> bool;
       fn remove(&self) -> Result<()>;
       fn cleanup(&self) -> Result<()>;
   }

   // Implement specific strategies
   struct LocalSkillRemove { skill_id: SkillId }
   struct RegistrySkillRemove { skill_id: SkillId }
   struct RemoteSkillRemove { skill_id: SkillId, url: String }
   ```

2. **Extract Validation Logic into Separate Functions**
   ```rust
   fn validate_remove_preconditions(skill: &Skill) -> Result<Vec<RemoveValidation>> {
       // Extract all validation checks
       // Returns collection of validation results
   }

   fn check_dependencies(skill: &Skill) -> Result<()> {
       // Extract dependency checking logic
   }

   fn check_active_usage(skill: &Skill) -> Result<()> {
       // Extract usage checking logic
   }
   ```

3. **Break Down execute_remove into Smaller Functions**
   ```rust
   fn execute_remove(args: RemoveArgs) -> Result<()> {
       let skill = find_skill(&args.skill_id)?;
       let validations = validate_remove_preconditions(&skill)?;

       if !validations.is_empty() {
           present_removal_options(&validations)?;
           if !confirm_removal()? {
               return Ok(());
           }
       }

       perform_removal(&skill, args.force)?;
       cleanup_after_removal(&skill)
   }

   fn perform_removal(skill: &Skill, force: bool) -> Result<()> {
       // Extract core removal logic
   }

   fn cleanup_after_removal(skill: &Skill) -> Result<()> {
       // Extract cleanup logic
   }
   ```

4. **Extract User Interaction Logic**
   ```rust
   fn present_removal_options(validations: &[RemoveValidation]) -> Result<()> {
       // Extract presentation logic
   }

   fn confirm_removal() -> Result<bool> {
       // Extract confirmation logic
   }

   fn select_removal_strategy(validations: &[RemoveValidation]) -> RemoveStrategy {
       // Extract strategy selection logic
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=35 -> cc=8-10
- **Nesting reduction**: 6 levels -> 2-3 levels
- **Method size**: 152 lines -> 30-50 lines per method
- **Bumpy road elimination**: 7 bumps -> 0 bumps
- **Target Score**: 7.99 -> 9.5+

---

## 2. cli/commands/publish.rs (Score: 7.39)

### Problems Identified

1. **Complex Method** (Severity: High)
   - Function: `execute_publish` (lines 43-118)
   - Complexity: cc=12 (recommended: <9)

2. **Deep Nested Complexity** (Severity: High)
   - Function: `find_zip_files` (lines 146-165)
   - Nesting depth: 4 conditionals (recommended: <4)

3. **Excess Number of Function Arguments** (Severity: Medium)
   - Function: `publish_to_api` (lines 168-272)
   - Arguments: 5 (recommended: <4)

4. **Large Method** (Severity: High)
   - Function: `publish_to_api` (lines 168-272)
   - Lines of code: 90 (recommended: <70)

5. **Code Duplication** (Severity: Medium)
   - Functions: `test_execute_publish_nonexistent_artifacts`, `test_execute_publish_empty_artifacts_dir`, `test_execute_publish_invalid_file_type`
   - Duplicate test setup and error handling

### Recommended Refactoring Actions

1. **Introduce Publish Context Struct**
   ```rust
   struct PublishContext {
       skill_name: String,
       version: String,
       artifacts_path: PathBuf,
       metadata: PublishMetadata,
       api_config: ApiConfig,
   }

   impl PublishContext {
       fn new(args: PublishArgs, config: &Config) -> Result<Self> {
           // Validate and create context
       }
   }
   ```

2. **Refactor publish_to_api Arguments**
   ```rust
   // Before: 5 arguments
   fn publish_to_api(
       client: &ApiClient,
       name: String,
       version: String,
       artifacts: Vec<PathBuf>,
       metadata: PublishMetadata,
   ) -> Result<()>

   // After: 1 argument (context)
   fn publish_to_api(context: &PublishContext, client: &ApiClient) -> Result<()> {
       // All data available through context
   }
   ```

3. **Extract Artifact Processing Logic**
   ```rust
   fn find_zip_files(path: &Path) -> Result<Vec<PathBuf>> {
       let mut zip_files = Vec::new();

       for entry in fs::read_dir(path)? {
           let entry = entry?;
           if entry.file_type()?.is_file() {
               let path = entry.path();
               if path.extension().map_or(false, |ext| ext == "zip") {
                   zip_files.push(path);
               }
           }
       }

       Ok(zip_files)
   }

   fn validate_artifacts(artifacts: &[PathBuf]) -> Result<()> {
       if artifacts.is_empty() {
           return Err(anyhow!("No zip artifacts found"));
       }

       for artifact in artifacts {
           validate_zip_file(artifact)?;
       }

       Ok(())
   }

   fn validate_zip_file(path: &Path) -> Result<()> {
       if !path.exists() {
           return Err(anyhow!("Artifact not found: {}", path.display()));
       }
       if path.extension().map_or(false, |ext| ext != "zip") {
           return Err(anyhow!("Invalid file type: {}", path.display()));
       }
       Ok(())
   }
   ```

4. **Break Down execute_publish**
   ```rust
   fn execute_publish(args: PublishArgs) -> Result<()> {
       let config = load_config()?;
       let context = PublishContext::new(args, &config)?;

       prepare_artifacts(&context)?;
       let artifacts = find_zip_files(&context.artifacts_path)?;
       validate_artifacts(&artifacts)?;

       publish_to_api(&context, &ApiClient::new(&config))?;
       cleanup_temp_files(&context)?;

       Ok(())
   }

   fn prepare_artifacts(context: &PublishContext) -> Result<()> {
       // Extract artifact preparation
   }

   fn cleanup_temp_files(context: &PublishContext) -> Result<()> {
       // Extract cleanup logic
   }
   ```

5. **Extract Test Helper Functions**
   ```rust
   #[cfg(test)]
   fn setup_publish_test(
       artifacts_dir: &str,
       artifacts: Vec<(&str, &[u8])>,
   ) -> (TempDir, PublishArgs) {
       let temp_dir = TempDir::new().unwrap();
       let artifacts_path = temp_dir.path().join(artifacts_dir);
       fs::create_dir_all(&artifacts_path).unwrap();

       for (name, contents) in artifacts {
           let path = artifacts_path.join(name);
           fs::write(path, contents).unwrap();
       }

       let args = PublishArgs {
           skill_name: "test-skill".to_string(),
           version: "1.0.0".to_string(),
           artifacts_path,
           ..Default::default()
       };

       (temp_dir, args)
   }

   #[test]
   fn test_execute_publish_nonexistent_artifacts() {
       let (_temp_dir, args) = setup_publish_test("artifacts", vec![]);
       let result = execute_publish(args);
       assert!(result.is_err());
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=12 -> cc=6-8
- **Nesting reduction**: 4 levels -> 2-3 levels
- **Method size**: 90 lines -> 40-50 lines
- **Arguments reduction**: 5 -> 1
- **Target Score**: 7.39 -> 9.0+

---

## 3. http/server.rs (Score: 7.93)

### Problems Identified

1. **Complex Method** (Severity: High)
   - Function: `validate_registry_config` (lines 58-121)
   - Complexity: cc=15 (recommended: <9)

2. **Bumpy Road Ahead** (Severity: High)
   - Function: `validate_registry_config` (lines 58-121)
   - Bumps: 2

3. **Large Methods** (Severity: Medium)
   - Function: `FastSkillServer.create_router` (lines 311-426)
   - Lines of code: 101 (recommended: <70)
   - Function: `build_cors_layer` (lines 130-234)
   - Lines of code: 95 (recommended: <70)

4. **Code Duplication** (Severity: Medium)
   - Functions: `FastSkillServer.new`, `FastSkillServer.from_ref`
   - Duplicate configuration setup

### Recommended Refactoring Actions

1. **Extract Validation Rules**
   ```rust
   trait RegistryConfigValidator {
       fn validate(&self, config: &RegistryConfig) -> Result<()>;
   }

   struct UrlValidator;
   impl RegistryConfigValidator for UrlValidator {
       fn validate(&self, config: &RegistryConfig) -> Result<()> {
           // Validate URL format and accessibility
       }
   }

   struct AuthValidator;
   impl RegistryConfigValidator for AuthValidator {
       fn validate(&self, config: &RegistryConfig) -> Result<()> {
           // Validate authentication credentials
       }
   }

   fn validate_registry_config(config: &RegistryConfig) -> Result<()> {
       let validators: Vec<Box<dyn RegistryConfigValidator>> = vec![
           Box::new(UrlValidator),
           Box::new(AuthValidator),
           // Add more validators as needed
       ];

       for validator in validators {
           validator.validate(config)?;
       }

       Ok(())
   }
   ```

2. **Break Down create_router**
   ```rust
   impl FastSkillServer {
       fn create_router(&self) -> Router {
           Router::new()
               .merge(self.create_skill_routes())
               .merge(self.create_registry_routes())
               .merge(self.create_auth_routes())
               .merge(self.create_search_routes())
               .merge(self.create_reindex_routes())
               .layer(self.build_middleware_layer())
       }

       fn create_skill_routes(&self) -> Router {
           Router::new()
               .route("/skills", get(handlers::list_skills))
               .route("/skills/:id", get(handlers::get_skill))
               .route("/skills/:id", delete(handlers::delete_skill))
       }

       fn create_registry_routes(&self) -> Router {
           Router::new()
               .route("/registry", get(handlers::get_registry_info))
               .route("/registry/publish", post(handlers::publish_to_registry))
       }

       fn create_auth_routes(&self) -> Router {
           Router::new()
               .route("/auth/login", post(handlers::auth::login))
               .route("/auth/refresh", post(handlers::auth::refresh_token))
       }

       fn create_search_routes(&self) -> Router {
           Router::new()
               .route("/search", get(handlers::search::search_skills))
       }

       fn create_reindex_routes(&self) -> Router {
           Router::new()
               .route("/reindex", post(handlers::reindex::trigger_reindex))
       }
   }
   ```

3. **Refactor build_cors_layer**
   ```rust
   struct CorsConfigBuilder {
       allowed_origins: Vec<String>,
       allowed_methods: Vec<Method>,
       allowed_headers: Vec<String>,
       allow_credentials: bool,
   }

   impl CorsConfigBuilder {
       fn new() -> Self {
           Self {
               allowed_origins: vec!["*".to_string()],
               allowed_methods: vec![Method::GET, Method::POST, Method::PUT, Method::DELETE],
               allowed_headers: vec![
                   "content-type".to_string(),
                   "authorization".to_string(),
               ],
               allow_credentials: false,
           }
       }

       fn with_origins(mut self, origins: Vec<String>) -> Self {
           self.allowed_origins = origins;
           self
       }

       fn with_credentials(mut self, allow: bool) -> Self {
           self.allow_credentials = allow;
           self
       }

       fn build(self) -> CorsLayer {
           CorsLayer::new()
               .allow_origin(self.allowed_origins)
               .allow_methods(self.allowed_methods)
               .allow_headers(self.allowed_headers)
               .allow_credentials(self.allow_credentials)
       }
   }

   fn build_cors_layer(config: &ServerConfig) -> CorsLayer {
       CorsConfigBuilder::new()
           .with_origins(config.allowed_origins.clone())
           .with_credentials(config.allow_credentials)
           .build()
   }
   ```

4. **Extract Common Constructor Logic**
   ```rust
   struct ServerBuilder {
       config: ServerConfig,
       service: Arc<FastSkillService>,
       registry: Arc<Registry>,
   }

   impl ServerBuilder {
       fn new(config: ServerConfig) -> Self {
           let service = Arc::new(FastSkillService::new(config.clone()));
           let registry = Arc::new(Registry::new(config.registry_config.clone()));

           Self { config, service, registry }
       }

       fn build(self) -> FastSkillServer {
           FastSkillServer {
               config: self.config,
               service: self.service,
               registry: self.registry,
               router: self.create_router(),
           }
       }
   }

   impl FastSkillServer {
       fn new(config: ServerConfig) -> Result<Self> {
           let builder = ServerBuilder::new(config);
           Ok(builder.build())
       }

       fn from_ref(config_ref: ConfigRef) -> Result<Self> {
           let config = ServerConfig::from_ref(config_ref)?;
           let builder = ServerBuilder::new(config);
           Ok(builder.build())
       }
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=15 -> cc=5-7
- **Method size**: 101 lines -> 15-25 lines per method, 95 lines -> 30-40 lines
- **Code duplication elimination**: Common logic extracted to builder pattern
- **Target Score**: 7.93 -> 9.2+

---

## 4. cli/commands/install.rs (Score: 8.13)

### Problems Identified

1. **Complex Method** (Severity: Critical)
   - Function: `execute_install` (lines 39-225)
   - Complexity: cc=19 (recommended: <9)
   - Function: `create_sources_manager_from_repositories` (lines 229-340)
   - Complexity: cc=11 (recommended: <9)

2. **Deep Nested Complexity** (Severity: High)
   - Function: `execute_install` (lines 39-225)
   - Nesting depth: 4 conditionals (recommended: <4)

3. **Bumpy Road Ahead** (Severity: High)
   - Function: `execute_install` (lines 39-225)
   - Bumps: 2

4. **Large Methods** (Severity: High)
   - Function: `execute_install` (lines 39-225)
   - Lines of code: 130 (recommended: <70)
   - Function: `create_sources_manager_from_repositories` (lines 229-340)
   - Lines of code: 98 (recommended: <70)

### Recommended Refactoring Actions

1. **Extract Install Strategy Pattern**
   ```rust
   enum InstallStrategy {
       Local { path: PathBuf },
       Registry { skill_id: SkillId, version: String },
       Git { url: String, branch: Option<String> },
   }

   impl InstallStrategy {
       fn from_args(args: &InstallArgs) -> Result<Self> {
           // Parse args into appropriate strategy
       }

       fn install(&self, service: &FastSkillService) -> Result<InstallResult> {
           match self {
               InstallStrategy::Local { path } => {
                   self.install_local(path, service)
               }
               InstallStrategy::Registry { skill_id, version } => {
                   self.install_from_registry(skill_id, version, service)
               }
               InstallStrategy::Git { url, branch } => {
                   self.install_from_git(url, branch, service)
               }
           }
       }

       fn install_local(&self, path: &Path, service: &FastSkillService) -> Result<InstallResult> {
           // Extract local install logic
       }

       fn install_from_registry(&self, skill_id: &str, version: &str, service: &FastSkillService) -> Result<InstallResult> {
           // Extract registry install logic
       }

       fn install_from_git(&self, url: &str, branch: &Option<String>, service: &FastSkillService) -> Result<InstallResult> {
           // Extract git install logic
       }
   }
   ```

2. **Break Down execute_install**
   ```rust
   fn execute_install(args: InstallArgs) -> Result<()> {
       let config = load_config()?;
       let service = FastSkillService::new(config)?;
       let strategy = InstallStrategy::from_args(&args)?;

       let result = strategy.install(&service)?;
       display_install_result(&result);

       Ok(())
   }

   fn display_install_result(result: &InstallResult) {
       // Extract display logic
   }
   ```

3. **Refactor Repository Management**
   ```rust
   struct RepositoryManagerBuilder {
       repositories: Vec<Repository>,
       config: Config,
   }

   impl RepositoryManagerBuilder {
       fn new(config: Config) -> Self {
           Self {
               repositories: vec![],
               config,
           }
       }

       fn add_local_repo(mut self, path: PathBuf) -> Result<Self> {
           let repo = Repository::local(path)?;
           self.repositories.push(repo);
           Ok(self)
       }

       fn add_git_repo(mut self, url: String) -> Result<Self> {
           let repo = Repository::git(url)?;
           self.repositories.push(repo);
           Ok(self)
       }

       fn add_registry_repo(mut self, url: String) -> Result<Self> {
           let repo = Repository::registry(url)?;
           self.repositories.push(repo);
           Ok(self)
       }

       fn build(self) -> SourcesManager {
           SourcesManager::new(self.repositories)
       }
   }

   fn create_sources_manager_from_repositories(config: &Config) -> Result<SourcesManager> {
       let mut builder = RepositoryManagerBuilder::new(config.clone());

       if let Some(local_path) = &config.local_repository_path {
           builder = builder.add_local_repo(local_path.clone())?;
       }

       if let Some(git_url) = &config.git_repository_url {
           builder = builder.add_git_repo(git_url.clone())?;
       }

       if let Some(registry_url) = &config.registry_url {
           builder = builder.add_registry_repo(registry_url.clone())?;
       }

       Ok(builder.build())
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=19 -> cc=6-8, cc=11 -> cc=4-6
- **Nesting reduction**: 4 levels -> 2-3 levels
- **Method size**: 130 lines -> 20-30 lines, 98 lines -> 15-20 lines
- **Target Score**: 8.13 -> 9.3+

---

## 5. cli/commands/list.rs (Score: 8.6)

### Problems Identified

1. **Complex Methods** (Severity: High)
   - Function: `execute_list` (lines 69-238)
   - Complexity: cc=21 (recommended: <9)
   - Function: `format_list_grid` (lines 241-346)
   - Complexity: cc=12 (recommended: <9)

2. **Large Methods** (Severity: High)
   - Function: `execute_list` (lines 69-238)
   - Lines of code: 140 (recommended: <70)
   - Function: `format_list_grid` (lines 241-346)
   - Lines of code: 98 (recommended: <70)

3. **Bumpy Road Ahead** (Severity: Medium)
   - Function: `format_list_grid` (lines 241-346)
   - Bumps: 2

### Recommended Refactoring Actions

1. **Extract Data Fetching Logic**
   ```rust
   struct SkillListFetcher {
       service: Arc<FastSkillService>,
       filter: ListFilter,
   }

   impl SkillListFetcher {
       fn new(service: Arc<FastSkillService>, filter: ListFilter) -> Self {
           Self { service, filter }
       }

       fn fetch(&self) -> Result<Vec<SkillListItem>> {
           let mut skills = self.service.list_skills()?;

           self.apply_filters(&mut skills);
           self.apply_sorting(&mut skills);

           Ok(skills)
       }

       fn apply_filters(&self, skills: &mut Vec<SkillListItem>) {
           // Extract filtering logic
       }

       fn apply_sorting(&self, skills: &mut Vec<SkillListItem>) {
           // Extract sorting logic
       }
   }
   ```

2. **Break Down execute_list**
   ```rust
   fn execute_list(args: ListArgs) -> Result<()> {
       let config = load_config()?;
       let service = FastSkillService::new(config)?;

       let filter = ListFilter::from_args(&args)?;
       let fetcher = SkillListFetcher::new(Arc::new(service), filter);

       let skills = fetcher.fetch()?;
       let formatted = format_skills(&skills, &args.format)?;
       println!("{}", formatted);

       Ok(())
   }

   fn format_skills(skills: &[SkillListItem], format: &ListFormat) -> Result<String> {
       match format {
           ListFormat::Table => format_as_table(skills),
           ListFormat::Json => format_as_json(skills),
           ListFormat::Grid => format_as_grid(skills),
       }
   }

   fn format_as_table(skills: &[SkillListItem]) -> Result<String> {
       // Extract table formatting
   }

   fn format_as_json(skills: &[SkillListItem]) -> Result<String> {
       // Extract JSON formatting
   }
   ```

3. **Refactor format_list_grid**
   ```rust
   struct GridFormatter {
       columns: Vec<GridColumn>,
       column_widths: Vec<usize>,
   }

   impl GridFormatter {
       fn new(columns: Vec<GridColumn>) -> Self {
           let column_widths = Self::calculate_widths(&columns);
           Self { columns, column_widths }
       }

       fn calculate_widths(columns: &[GridColumn]) -> Vec<usize> {
           // Calculate optimal column widths
       }

       fn format(&self, skills: &[SkillListItem]) -> String {
           let mut output = String::new();
           self.format_header(&mut output);
           self.format_separator(&mut output);
           self.format_rows(&mut output, skills);
           output
       }

       fn format_header(&self, output: &mut String) {
           // Format header row
       }

       fn format_separator(&self, output: &mut String) {
           // Format separator row
       }

       fn format_rows(&self, output: &mut String, skills: &[SkillListItem]) {
           // Format data rows
       }
   }

   fn format_list_grid(skills: &[SkillListItem]) -> Result<String> {
       let columns = vec![
           GridColumn::new("Name", |s: &SkillListItem| s.name.clone()),
           GridColumn::new("Version", |s: &SkillListItem| s.version.clone()),
           GridColumn::new("Source", |s: &SkillListItem| s.source.clone()),
       ];

       let formatter = GridFormatter::new(columns);
       Ok(formatter.format(skills))
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=21 -> cc=5-7, cc=12 -> cc=4-6
- **Method size**: 140 lines -> 20-30 lines, 98 lines -> 40-50 lines
- **Bumpy road elimination**: 2 bumps -> 0 bumps
- **Target Score**: 8.6 -> 9.4+

---

## 6. validation/standard_validator.rs (Score: 8.54)

### Problems Identified

1. **Bumpy Road Ahead** (Severity: Medium)
   - Function: `validate_file_references` (lines 158-200)
   - Bumps: 2
   - Function: `validate_directory_structure` (lines 203-227)
   - Bumps: 2

2. **Deep Nested Complexity** (Severity: Medium)
   - Function: `validate_file_references` (lines 158-200)
   - Nesting depth: 4 conditionals (recommended: <4)

3. **Code Duplication** (Severity: Low)
   - Test functions: `test_validate_skill_directory_valid`, `test_validate_skill_directory_missing_required`, `test_validate_file_references_missing`

### Recommended Refactoring Actions

1. **Extract File Reference Validation**
   ```rust
   struct FileReferenceValidator {
       skill_path: PathBuf,
   }

   impl FileReferenceValidator {
       fn new(skill_path: PathBuf) -> Self {
           Self { skill_path }
       }

       fn validate(&self, references: &[&str]) -> Result<ValidationResult> {
           let mut result = ValidationResult::new();

           for reference in references {
               match self.validate_reference(reference) {
                   Ok(_) => result.add_passed(reference),
                   Err(e) => result.add_failed(reference, e),
               }
           }

           Ok(result)
       }

       fn validate_reference(&self, reference: &str) -> Result<()> {
           let path = self.skill_path.join(reference);
           if !path.exists() {
               return Err(anyhow!("Referenced file not found: {}", reference));
           }
           Ok(())
       }
   }
   ```

2. **Break Down validate_file_references**
   ```rust
   fn validate_file_references(&self) -> Result<ValidationResult> {
       let validator = FileReferenceValidator::new(self.skill_path.clone());

       let script_refs = self.get_script_references();
       let reference_refs = self.get_reference_references();

       let script_result = validator.validate(&script_refs)?;
       let reference_result = validator.validate(&reference_refs)?;

       Ok(ValidationResult::merge(vec![script_result, reference_result]))
   }

   fn get_script_references(&self) -> Vec<&str> {
       // Extract script reference gathering
   }

   fn get_reference_references(&self) -> Vec<&str> {
       // Extract reference reference gathering
   }
   ```

3. **Simplify validate_directory_structure**
   ```rust
   fn validate_directory_structure(&self) -> Result<ValidationResult> {
       let required = self.get_required_directories();
       let optional = self.get_optional_directories();

       let mut result = ValidationResult::new();

       for dir in &required {
           self.validate_directory_exists(dir, true, &mut result);
       }

       for dir in &optional {
           self.validate_directory_exists(dir, false, &mut result);
       }

       Ok(result)
   }

   fn validate_directory_exists(&self, dir_name: &str, required: bool, result: &mut ValidationResult) {
       let path = self.skill_path.join(dir_name);
       if path.exists() {
           result.add_passed(dir_name);
       } else if required {
           result.add_failed(dir_name, anyhow!("Required directory missing: {}", dir_name));
       }
   }
   ```

4. **Extract Test Helpers**
   ```rust
   #[cfg(test)]
   fn create_test_skill_dir(name: &str) -> TempDir {
       let temp_dir = TempDir::new().unwrap();
       let skill_path = temp_dir.path().join(name);
       fs::create_dir_all(&skill_path).unwrap();
       temp_dir
   }

   #[cfg(test)]
   fn create_required_directories(base: &Path) {
       fs::create_dir_all(base.join("scripts")).unwrap();
       fs::create_dir_all(base.join("references")).unwrap();
   }

   #[test]
   fn test_validate_skill_directory_valid() {
       let temp_dir = create_test_skill_dir("test_skill");
       create_required_directories(temp_dir.path());

       let validator = StandardValidator::new(temp_dir.path().to_path_buf());
       let result = validator.validate_directory_structure().unwrap();

       assert!(result.all_passed());
   }
   ```

### Expected Impact

- **Nesting reduction**: 4 levels -> 2-3 levels
- **Bumpy road elimination**: 2 bumps -> 0 bumps
- **Code duplication elimination**: Test helpers extracted
- **Target Score**: 8.54 -> 9.5+

---

## 7. storage/git.rs (Score: 9.09)

### Problems Identified

1. **Complex Method** (Severity: Medium)
   - Function: `execute_git_command_with_retry` (lines 242-298)
   - Complexity: cc=9 (exceeds threshold by 1)

2. **Complex Conditional** (Severity: Low)
   - Function: `parse_git_version` (line 142)
   - 2 complex conditional expressions
   - Function: `execute_git_command_with_retry` (lines 280-281)
   - 2 complex conditional expressions

### Recommended Refactoring Actions

1. **Extract Retry Logic**
   ```rust
   struct GitRetryPolicy {
       max_attempts: u32,
       retry_delay: Duration,
       retryable_errors: Vec<String>,
   }

   impl GitRetryPolicy {
       fn default() -> Self {
           Self {
               max_attempts: 3,
               retry_delay: Duration::from_secs(1),
               retryable_errors: vec![
                   "network".to_string(),
                   "timeout".to_string(),
                   "connection".to_string(),
               ],
           }
       }

       fn should_retry(&self, error: &str, attempt: u32) -> bool {
           if attempt >= self.max_attempts {
               return false;
           }
           self.retryable_errors.iter()
               .any(|pattern| error.to_lowercase().contains(pattern))
       }
   }

   fn execute_git_command_with_retry(
       command: &str,
       args: &[&str],
       policy: &GitRetryPolicy,
   ) -> Result<String> {
       for attempt in 0..policy.max_attempts {
           match execute_git_command(command, args) {
               Ok(output) => return Ok(output),
               Err(e) => {
                   let error_msg = e.to_string();
                   if policy.should_retry(&error_msg, attempt) {
                       thread::sleep(policy.retry_delay);
                       continue;
                   }
                   return Err(e);
               }
           }
       }
       Err(anyhow!("Max retry attempts exceeded"))
   }
   ```

2. **Extract Complex Conditionals**
   ```rust
   fn parse_git_version(version_str: &str) -> Result<(u32, u32, u32)> {
       let cleaned = clean_version_string(version_str)?;
       let parts: Vec<&str> = cleaned.split('.').collect();

       if parts.len() < 3 {
           return Err(anyhow!("Invalid version format"));
       }

       let major = parts[0].parse()?;
       let minor = parts[1].parse()?;
       let patch = parts[2].parse()?;

       Ok((major, minor, patch))
   }

   fn clean_version_string(version_str: &str) -> Result<String> {
       let cleaned = version_str
           .trim()
           .trim_start_matches("git version ")
           .trim_start_matches('v')
           .split_whitespace()
           .next()
           .ok_or_else(|| anyhow!("Empty version string"))?
           .to_string();

       if cleaned.is_empty() {
           return Err(anyhow!("Empty version after cleaning"));
       }

       Ok(cleaned)
   }

   fn should_retry_git_operation(error: &Error, attempt: u32, max_attempts: u32) -> bool {
       if attempt >= max_attempts {
           return false;
       }

       let error_msg = error.to_string().to_lowercase();
       error_msg.contains("network")
           || error_msg.contains("timeout")
           || error_msg.contains("connection")
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=9 -> cc=4-6
- **Complex conditional elimination**: Encapsulated in helper functions
- **Target Score**: 9.09 -> 9.6+

---

## 8. cli/commands/sync.rs (Score: 9.54)

### Problems Identified

1. **Large Method** (Severity: Low)
   - Function: `execute_sync` (lines 34-138)
   - Lines of code: 81 (exceeds threshold by 11)

### Recommended Refactoring Actions

1. **Extract Sync Steps**
   ```rust
   struct SyncExecutor {
       service: Arc<FastSkillService>,
       config: SyncConfig,
   }

   impl SyncExecutor {
       fn new(service: Arc<FastSkillService>, config: SyncConfig) -> Self {
           Self { service, config }
       }

       fn execute(&self) -> Result<SyncResult> {
           let pre_sync = self.prepare_sync()?;
           let sync_result = self.perform_sync(&pre_sync)?;
           let post_sync = self.finalize_sync(&sync_result)?;

           Ok(SyncResult {
               pre_sync,
               sync_result,
               post_sync,
           })
       }

       fn prepare_sync(&self) -> Result<SyncPreState> {
           // Extract pre-sync logic
       }

       fn perform_sync(&self, state: &SyncPreState) -> Result<SyncActionResult> {
           // Extract sync logic
       }

       fn finalize_sync(&self, result: &SyncActionResult) -> Result<SyncPostState> {
           // Extract post-sync logic
       }
   }

   fn execute_sync(args: SyncArgs) -> Result<()> {
       let config = load_config()?;
       let service = FastSkillService::new(config)?;

       let sync_config = SyncConfig::from_args(&args)?;
       let executor = SyncExecutor::new(Arc::new(service), sync_config);

       let result = executor.execute()?;
       display_sync_result(&result);

       Ok(())
   }

   fn display_sync_result(result: &SyncResult) {
       // Extract display logic
   }
   ```

### Expected Impact

- **Method size**: 81 lines -> 20-30 lines
- **Target Score**: 9.54 -> 9.8+

---

## 9. validation/skill_validator.rs (Score: 9.38)

### Problems Identified

1. **Code Duplication** (Severity: Low)
   - Functions: `validate_scripts_directory`, `validate_references_directory`
   - Similar structure and logic

### Recommended Refactoring Actions

1. **Extract Common Validation Logic**
   ```rust
   fn validate_directory(
       &self,
       directory_name: &str,
       required_files: &[&str],
       optional_files: &[&str],
   ) -> Result<ValidationResult> {
       let dir_path = self.skill_path.join(directory_name);
       let mut result = ValidationResult::new();

       if !dir_path.exists() {
           result.add_info(format!("Directory {} not found (optional)", directory_name));
           return Ok(result);
       }

       self.validate_required_files(&dir_path, required_files, &mut result);
       self.validate_optional_files(&dir_path, optional_files, &mut result);

       Ok(result)
   }

   fn validate_required_files(
       &self,
       dir_path: &Path,
       files: &[&str],
       result: &mut ValidationResult,
   ) {
       for file in files {
           let file_path = dir_path.join(file);
           if file_path.exists() {
               result.add_passed(file);
           } else {
               result.add_failed(file, anyhow!("Required file not found"));
           }
       }
   }

   fn validate_optional_files(
       &self,
       dir_path: &Path,
       files: &[&str],
       result: &mut ValidationResult,
   ) {
       for file in files {
           let file_path = dir_path.join(file);
           if file_path.exists() {
               result.add_passed(file);
           } else {
               result.add_info(format!("Optional file not found: {}", file));
           }
       }
   }

   fn validate_scripts_directory(&self) -> Result<ValidationResult> {
       self.validate_directory(
           "scripts",
           &["install.sh"],
           &["uninstall.sh", "upgrade.sh"],
       )
   }

   fn validate_references_directory(&self) -> Result<ValidationResult> {
       self.validate_directory(
           "references",
           &["README.md"],
           &["CONTRIBUTING.md", "LICENSE"],
       )
   }
   ```

### Expected Impact

- **Code duplication elimination**: Single validation function
- **Maintainability improvement**: Easier to add new validations
- **Target Score**: 9.38 -> 9.7+

---

## 10. storage/filesystem.rs (Score: 9.38)

### Problems Identified

1. **Deep Nested Complexity** (Severity: Medium)
   - Function: `list_skill_ids` (lines 171-199)
   - Nesting depth: 4 conditionals (recommended: <4)

### Recommended Refactoring Actions

1. **Extract Directory Iteration**
   ```rust
   fn list_skill_ids(&self) -> Result<Vec<SkillId>> {
       let entries = self.read_skill_directory_entries()?;
       let skill_ids: Result<Vec<_>> = entries
           .filter_map(|entry| self.try_parse_skill_id(entry))
           .collect();

       skill_ids
   }

   fn read_skill_directory_entries(&self) -> Result<Vec<DirEntry>> {
       fs::read_dir(&self.skills_path)?
           .collect::<Result<Vec<_>, _>>()
           .map_err(|e| anyhow!("Failed to read skills directory: {}", e))
   }

   fn try_parse_skill_id(&self, entry: DirEntry) -> Option<Result<SkillId>> {
       let file_type = self.get_entry_type(&entry).ok()?;
       if !self.is_skill_directory(&file_type) {
           return None;
       }

       let skill_id = self.parse_skill_id_from_entry(entry).ok()?;
       Some(Ok(skill_id))
   }

   fn get_entry_type(&self, entry: &DirEntry) -> Result<FileType> {
       entry.file_type()
           .map_err(|e| anyhow!("Failed to get file type: {}", e))
   }

   fn is_skill_directory(&self, file_type: &FileType) -> bool {
       file_type.is_dir()
   }

   fn parse_skill_id_from_entry(&self, entry: DirEntry) -> Result<SkillId> {
       let path = entry.path();
       let file_name = path.file_name()
           .and_then(|name| name.to_str())
           .ok_or_else(|| anyhow!("Invalid directory name"))?;

       SkillId::new(file_name)
   }
   ```

### Expected Impact

- **Nesting reduction**: 4 levels -> 1-2 levels
- **Readability improvement**: Clear, linear flow
- **Testability improvement**: Each function independently testable
- **Target Score**: 9.38 -> 9.7+

---

## 11. core/service.rs (Score: 9.38)

### Problems Identified

1. **Complex Method** (Severity: Medium)
   - Function: `auto_index_skills_from_filesystem` (lines 500-547)
   - Complexity: cc=9 (exceeds threshold by 1)

2. **Complex Conditional** (Severity: Low)
   - Function: `SkillId.new` (lines 192-194)
   - 2 complex conditional expressions

### Recommended Refactoring Actions

1. **Extract Indexing Logic**
   ```rust
   struct FilesystemSkillIndexer {
       service: Arc<FastSkillService>,
   }

   impl FilesystemSkillIndexer {
       fn new(service: Arc<FastSkillService>) -> Self {
           Self { service }
       }

       fn index_from_filesystem(&self) -> Result<IndexResult> {
           let discovered_skills = self.discover_skills()?;
           let indexed_skills = self.index_skills(discovered_skills)?;

           Ok(IndexResult {
               discovered: discovered_skills.len(),
               indexed: indexed_skills.len(),
               failed: discovered_skills.len() - indexed_skills.len(),
           })
       }

       fn discover_skills(&self) -> Result<Vec<SkillMetadata>> {
           let storage = self.service.get_storage();
           storage.list_skill_ids()?
               .into_iter()
               .filter_map(|skill_id| self.load_skill_metadata(&skill_id).ok())
               .collect()
       }

       fn index_skills(&self, skills: Vec<SkillMetadata>) -> Result<Vec<SkillMetadata>> {
           skills.into_iter()
               .filter_map(|metadata| self.index_single_skill(metadata).ok())
               .collect()
       }

       fn index_single_skill(&self, metadata: SkillMetadata) -> Result<SkillMetadata> {
           self.service.index_skill(metadata)
       }
   }

   impl FastSkillService {
       fn auto_index_skills_from_filesystem(&self) -> Result<IndexResult> {
           let indexer = FilesystemSkillIndexer::new(Arc::new(self.clone()));
           indexer.index_from_filesystem()
       }
   }
   ```

2. **Simplify SkillId Validation**
   ```rust
   impl SkillId {
       pub fn new(id: &str) -> Result<Self> {
           let cleaned = self::clean_skill_id(id)?;
           Self::validate_skill_id(&cleaned)?;

           Ok(SkillId { id: cleaned })
       }

       fn clean_skill_id(id: &str) -> Result<String> {
           let cleaned = id.trim().to_lowercase();
           if cleaned.is_empty() {
               return Err(anyhow!("Skill ID cannot be empty"));
           }
           Ok(cleaned)
       }

       fn validate_skill_id(id: &str) -> Result<()> {
           if !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
               return Err(anyhow!("Skill ID contains invalid characters"));
           }
           if id.len() > 100 {
               return Err(anyhow!("Skill ID too long (max 100 characters)"));
           }
           Ok(())
       }
   }
   ```

### Expected Impact

- **Complexity reduction**: cc=9 -> cc=3-5
- **Complex conditional elimination**: Encapsulated in validation function
- **Target Score**: 9.38 -> 9.7+

---

## 12. execution.rs (Score: 9.84)

### Problems Identified

1. **Bumpy Road Ahead** (Severity: Low)
   - Function: `validate_script` (lines 239-297)
   - Bumps: 2

### Recommended Refactoring Actions

1. **Extract Validation Rules**
   ```rust
   struct ScriptValidator {
       rules: Vec<Box<dyn ScriptValidationRule>>,
   }

   impl ScriptValidator {
       fn new() -> Self {
           Self {
               rules: vec![
                   Box::new(FileExtensionRule),
                   Box::new(FileSizeRule),
                   Box::new(ShebangRule),
                   Box::new(PermissionRule),
               ],
           }
       }

       fn validate(&self, script: &Path) -> Result<ValidationResult> {
           let mut result = ValidationResult::new();

           for rule in &self.rules {
               match rule.validate(script) {
                   Ok(_) => result.add_passed(rule.name()),
                   Err(e) => result.add_failed(rule.name(), e),
               }
           }

           Ok(result)
       }
   }

   trait ScriptValidationRule {
       fn validate(&self, script: &Path) -> Result<()>;
       fn name(&self) -> &str;
   }

   struct FileExtensionRule;
   impl ScriptValidationRule for FileExtensionRule {
       fn validate(&self, script: &Path) -> Result<()> {
           let ext = script.extension()
               .and_then(|e| e.to_str())
               .ok_or_else(|| anyhow!("No file extension"))?;

           if !matches!(ext, "sh" | "py" | "rb" | "js" | "ts") {
               return Err(anyhow!("Unsupported script extension: {}", ext));
           }

           Ok(())
       }

       fn name(&self) -> &str { "file_extension" }
   }

   struct FileSizeRule;
   impl ScriptValidationRule for FileSizeRule {
       fn validate(&self, script: &Path) -> Result<()> {
           let metadata = fs::metadata(script)?;
           let size = metadata.len();

           if size > 1024 * 1024 { // 1MB limit
               return Err(anyhow!("Script too large: {} bytes", size));
           }

           Ok(())
       }

       fn name(&self) -> &str { "file_size" }
   }

   struct ShebangRule;
   impl ScriptValidationRule for ShebangRule {
       fn validate(&self, script: &Path) -> Result<()> {
           let content = fs::read_to_string(script)?;
           if !content.starts_with("#!") {
               return Err(anyhow!("Missing shebang line"));
           }
           Ok(())
       }

       fn name(&self) -> &str { "shebang" }
   }

   struct PermissionRule;
   impl ScriptValidationRule for PermissionRule {
       fn validate(&self, script: &Path) -> Result<()> {
           let metadata = fs::metadata(script)?;
           let permissions = metadata.permissions();
           let mode = permissions.mode();

           if mode & 0o111 == 0 {
               return Err(anyhow!("Script not executable"));
           }

           Ok(())
       }

       fn name(&self) -> &str { "permissions" }
   }

   impl ExecutionSandbox {
       fn validate_script(&self, script: &Path) -> Result<()> {
           let validator = ScriptValidator::new();
           let result = validator.validate(script)?;

           if !result.all_passed() {
               return Err(anyhow!("Script validation failed: {:?}", result));
           }

           Ok(())
       }
   }
   ```

### Expected Impact

- **Bumpy road elimination**: 2 bumps -> 0 bumps
- **Extensibility**: Easy to add new validation rules
- **Target Score**: 9.84 -> 9.9+

---

## Implementation Priority

### Phase 1: Critical Issues (Week 1-2)

1. **cli/commands/remove.rs** (Score: 7.99)
   - Highest priority: cc=35, 6-level nesting, 7 bumpy road sections
   - Estimated effort: 8-12 hours
   - Target score: 9.5+

2. **cli/commands/publish.rs** (Score: 7.39)
   - High priority: Multiple issues including code duplication
   - Estimated effort: 6-10 hours
   - Target score: 9.0+

3. **http/server.rs** (Score: 7.93)
   - High priority: Complex configuration validation
   - Estimated effort: 6-8 hours
   - Target score: 9.2+

### Phase 2: High Priority (Week 3-4)

4. **cli/commands/install.rs** (Score: 8.13)
   - Estimated effort: 6-10 hours
   - Target score: 9.3+

5. **cli/commands/list.rs** (Score: 8.6)
   - Estimated effort: 4-6 hours
   - Target score: 9.4+

6. **validation/standard_validator.rs** (Score: 8.54)
   - Estimated effort: 4-6 hours
   - Target score: 9.5+

### Phase 3: Medium Priority (Week 5-6)

7. **storage/git.rs** (Score: 9.09)
   - Estimated effort: 3-4 hours
   - Target score: 9.6+

8. **cli/commands/sync.rs** (Score: 9.54)
   - Estimated effort: 2-3 hours
   - Target score: 9.8+

9. **validation/skill_validator.rs** (Score: 9.38)
   - Estimated effort: 2-3 hours
   - Target score: 9.7+

10. **storage/filesystem.rs** (Score: 9.38)
    - Estimated effort: 2-3 hours
    - Target score: 9.7+

11. **core/service.rs** (Score: 9.38)
    - Estimated effort: 3-4 hours
    - Target score: 9.7+

12. **execution.rs** (Score: 9.84)
    - Estimated effort: 2-3 hours
    - Target score: 9.9+

## General Refactoring Patterns

### 1. Extract Method Pattern
Break large functions into smaller, focused methods:
- Single Responsibility Principle
- Each method does one thing well
- Descriptive names that indicate intent

### 2. Strategy Pattern
Replace complex conditional logic with strategy objects:
- Each strategy encapsulates a behavior
- Easy to add new strategies
- Reduces complexity

### 3. Builder Pattern
Simplify complex object construction:
- Fluent interface
- Optional parameters
- Validates at construction time

### 4. Command Pattern
Encapsulate actions as objects:
- Decouples invoker from receiver
- Supports undo/redo
- Parameterizable

### 5. Extract Interface
Create abstraction for dependencies:
- Improves testability
- Enables mocking
- Supports different implementations

## Testing Strategy

1. **Before Refactoring**
   - Ensure all existing tests pass
   - Run CodeScene to get baseline scores
   - Document current behavior

2. **During Refactoring**
   - Write unit tests for extracted functions
   - Maintain test coverage
   - Run tests after each change

3. **After Refactoring**
   - Verify all tests pass
   - Run CodeScene to verify score improvements
   - Update documentation as needed

## Success Metrics

- All files achieve score > 8.0
- Total cyclomatic complexity reduced by 40%+
- Average nesting depth reduced to < 3
- Code duplication reduced by 50%+
- Average method length < 50 lines
- Test coverage maintained or improved

## Implementation Guide

### Prerequisites

Before starting any refactoring work, ensure you have:

1. **Development Environment Setup**
   - Rust nightly toolchain installed (see `rust-toolchain.toml`)
   - `cargo-nextest` installed: `cargo install cargo-nextest`
   - `cargo-insta` installed: `cargo install cargo-insta`
   - `typos` installed for spell checking
   - `cargo-shear` installed for unused dependency detection
   - Access to CodeScene or equivalent code quality analysis tool

2. **Baseline Validation**
   ```bash
   # Ensure all tests pass before starting
   cargo nextest run --all-features

   # Verify code compiles without warnings
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

   # Check formatting
   cargo fmt --all -- --check

   # Document current CodeScene scores
   # Run CodeScene analysis and save results for comparison
   ```

3. **Repository Access**
   - Write access to the fastskill repository
   - Familiarity with the project's branching strategy (works from `main` branch)
   - Understanding of PR review process

4. **Context & Documentation**
   - Read `CLAUDE.md` for project conventions
   - Read `STYLE.md` for coding style guidelines
   - Review `CONTRIBUTING.md` for contribution workflow
   - Understand async patterns and error handling conventions

### Step-by-Step Workflow

#### 1. Pre-Refactoring Phase

```bash
# Create feature branch
git checkout main
git pull origin main
git checkout -b refactor/<component>-<issue-summary>

# Example: git checkout -b refactor/remove-command-complexity
```

**Verify Current State:**
- Check that line numbers in this plan match current code (files may have changed)
- If line numbers are outdated, locate the functions by name instead
- Run CodeScene or equivalent to confirm current scores match this plan
- Document any discrepancies

#### 2. Refactoring Phase

**For Each File Being Refactored:**

a. **Read and Understand**
   ```bash
   # Read the file thoroughly
   # Understand current behavior and test coverage
   # Identify all callers of functions being modified
   ```

b. **Check Test Coverage**
   ```bash
   # Run tests for this specific module
   cargo nextest run -E 'test(module_name)'

   # If tests don't exist, write them FIRST before refactoring
   # This ensures you can verify behavior preservation
   ```

c. **Apply Refactoring**
   - Start with smallest extraction first (helper functions)
   - Test after each extraction
   - Use recommended patterns from this plan
   - Maintain identical external behavior (no functional changes)

d. **Validate After Each Change**
   ```bash
   # Run tests
   cargo nextest run

   # Check for new warnings
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

   # Format code
   cargo fmt --all

   # Review snapshots if CLI output changed
   cargo insta review

   # Accept snapshots only if changes are expected
   cargo insta accept
   ```

e. **Commit Incremental Progress**
   ```bash
   # Make small, logical commits
   git add <files>
   git commit -m "refactor(cli): extract validation logic from remove command"

   # Follow conventional commit format (see CLAUDE.md)
   # Do NOT add Co-authored-by trailers for AI tools
   ```

#### 3. Validation Phase

**Before Creating PR:**

```bash
# Run full test suite
cargo nextest run --all-features

# Run all quality checks
cargo fmt --all
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
typos
cargo shear

# Verify no unused dependencies introduced
# Verify all snapshot tests pass
cargo insta test --accept --test-runner nextest

# Run in release mode to catch optimization issues
cargo build --release
```

**Measure Improvement:**
- Re-run CodeScene analysis on refactored files
- Verify cyclomatic complexity decreased
- Verify nesting depth decreased
- Verify method length decreased
- Document before/after metrics for PR description

**Expected Metrics:**
- Use the "Expected Impact" section from each file's refactoring plan
- If metrics don't improve as expected, review the refactoring
- Consider whether additional changes are needed

#### 4. PR Creation Phase

**Branch and PR Guidelines:**

1. **One file or closely-related module per PR** (keep changes focused)
   - Exception: If two files have identical patterns, can combine
   - Example: `cli/commands/remove.rs` should be separate from `cli/commands/publish.rs`

2. **PR Title Format:**
   ```
   refactor(cli): reduce complexity in remove command
   refactor(http): extract validation rules in server config
   refactor(validation): eliminate code duplication
   ```

3. **PR Description Template:**
   ```markdown
   ## Summary
   Refactors [file] to improve code health score from X.XX to Y.YY

   ## Changes Made
   - Extracted [function/pattern] to reduce complexity
   - Reduced cyclomatic complexity from X to Y
   - Reduced nesting depth from X to Y
   - Reduced method length from X to Y lines

   ## Before/After Metrics
   | Metric | Before | After | Target |
   |--------|--------|-------|--------|
   | Score | X.XX | Y.YY | Z.ZZ |
   | Complexity | XX | YY | <9 |
   | Nesting | X | Y | <4 |
   | Method Length | XXX | YY | <70 |

   ## Testing
   - [ ] All existing tests pass
   - [ ] Added tests for extracted functions
   - [ ] Snapshot tests reviewed and accepted
   - [ ] No clippy warnings
   - [ ] Code coverage maintained or improved

   ## References
   - CodeScene Health Improvement Plan: Section X
   ```

4. **Request Review:**
   - Tag appropriate reviewers based on component
   - Reference this plan document in PR
   - Be prepared to explain refactoring decisions

#### 5. Post-Merge Phase

```bash
# After PR merged, update local main
git checkout main
git pull origin main

# Delete feature branch
git branch -d refactor/<component>-<issue-summary>

# Update tracking document (if maintained)
# Mark file as completed in implementation tracker
```

### How to Measure Success

#### Automated Metrics

1. **Test Suite:**
   ```bash
   # All tests must pass
   cargo nextest run --all-features

   # Check test coverage (if using cargo-tarpaulin)
   cargo tarpaulin --all-features --workspace --timeout 300
   ```

2. **Code Quality:**
   ```bash
   # No clippy warnings
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

   # Properly formatted
   cargo fmt --all -- --check

   # No typos
   typos

   # No unused dependencies
   cargo shear
   ```

#### Manual Validation

1. **CodeScene Re-Analysis:**
   - Run CodeScene on refactored files
   - Compare scores to baseline and targets
   - Document improvements in tracking spreadsheet/document

2. **Code Review Feedback:**
   - Address all reviewer comments
   - Ensure team agrees refactoring improves readability
   - Verify no unintended behavior changes

3. **Functional Testing:**
   ```bash
   # For CLI commands, test manually
   cargo run --bin fastskill -- <command> <args>

   # Example: For remove.rs refactoring
   cargo run --bin fastskill -- remove test-skill
   cargo run --bin fastskill -- remove test-skill --force

   # Verify output matches expected behavior
   ```

#### Success Criteria Checklist

For each refactored file:

- [ ] CodeScene score improved to target (see Expected Impact sections)
- [ ] All automated tests pass
- [ ] No new clippy warnings introduced
- [ ] Code coverage maintained or improved (>= baseline)
- [ ] Cyclomatic complexity reduced per targets
- [ ] Nesting depth reduced to ≤ 4 levels
- [ ] Method length reduced to ≤ 70 lines
- [ ] No functional regressions (behavior unchanged)
- [ ] PR approved and merged
- [ ] Documentation updated if needed

### Coordination and Ownership

#### Team Coordination

1. **Work Assignment:**
   - Use project tracking tool (GitHub Projects, Jira, etc.)
   - Assign one developer per file to avoid conflicts
   - Phase 1 tasks can be parallelized across 3 developers
   - Phase 2 and 3 can start once Phase 1 completes

2. **Communication Channels:**
   - Daily standup: Report progress on current refactoring
   - PR reviews: Tag team members familiar with the component
   - Blockers: Escalate if refactoring reveals deeper issues
   - Questions: Discuss architectural decisions before implementing

3. **Conflict Prevention:**
   - Check assigned tasks before starting work
   - Communicate if planning to work on a file
   - Rebase frequently to avoid merge conflicts
   - Coordinate if two files are tightly coupled

4. **Ownership Model:**
   - **Phase 1 (Critical):** Senior developers with codebase expertise
   - **Phase 2 (High Priority):** Mid-level developers familiar with domain
   - **Phase 3 (Medium Priority):** Can be distributed across team
   - **Review:** Each PR reviewed by at least one other team member

#### Progress Tracking

Create a tracking document (e.g., GitHub Project, spreadsheet):

| File | Phase | Assignee | Status | Branch | PR | Score Before | Score After | Completed Date |
|------|-------|----------|--------|--------|-----|--------------|-------------|----------------|
| cli/commands/remove.rs | 1 | Alice | In Progress | refactor/remove-complexity | #123 | 7.99 | - | - |
| cli/commands/publish.rs | 1 | Bob | Not Started | - | - | 7.39 | - | - |

**Status Values:**
- Not Started
- In Progress
- In Review
- Completed
- Blocked (with reason)

### Edge Cases and Troubleshooting

#### What if tests fail after refactoring?

1. **Identify the failure:**
   ```bash
   # Run tests with verbose output
   cargo nextest run --no-fail-fast

   # For specific failing test
   cargo nextest run -E 'test(failing_test_name)' -- --nocapture
   ```

2. **Common causes:**
   - Extracted function has different behavior (logic error)
   - Test was testing implementation details, not behavior
   - Snapshot test needs updating (CLI output changed)
   - Async timing issues (if refactoring changed async boundaries)

3. **Resolution:**
   - Revert the change that broke the test
   - Understand why test failed (behavior change or implementation coupling?)
   - Fix the logic error or update the test appropriately
   - Only update snapshots if output changes are intentional and correct

#### What if complexity doesn't improve as expected?

1. **Verify metrics:**
   - Manually count cyclomatic complexity
   - Check if CodeScene/tool is measuring correctly
   - Ensure refactoring actually applied the recommended pattern

2. **Possible reasons:**
   - Refactoring was partial (didn't extract enough)
   - New code introduced different complexity
   - Tool measures differently than expected

3. **Resolution:**
   - Review the recommended refactoring again
   - Consider more aggressive extraction
   - Discuss with team if target is realistic
   - Document why target couldn't be met (for retrospective)

#### What if refactoring reveals deeper architectural issues?

1. **Examples:**
   - Circular dependencies
   - Tight coupling that prevents extraction
   - Missing abstractions
   - Unclear ownership of responsibilities

2. **Response:**
   - Document the issue in a new ticket
   - Discuss with team architect or lead
   - Decide if issue should be addressed now or deferred
   - If deferring, document technical debt
   - May need to revise refactoring approach

3. **Decision framework:**
   - If fix is < 2x the estimated refactoring time → address now
   - If fix is > 2x → create ticket, defer, continue with simpler refactoring
   - If blocking → escalate to tech lead

#### What if snapshot tests fail?

1. **Review changes:**
   ```bash
   # Review what changed
   cargo insta review

   # See specific differences
   # Insta will show old vs new snapshots
   ```

2. **Determine if changes are acceptable:**
   - Is the new output correct?
   - Did refactoring intentionally change formatting?
   - Is this a regression or improvement?

3. **Actions:**
   ```bash
   # If changes are correct
   cargo insta accept

   # If changes are wrong
   # Revert code changes and fix the issue

   # If changes are unexpected
   # Investigate why output changed
   ```

#### What if PR review requests changes?

1. **Common review feedback:**
   - Extraction went too far (over-abstraction)
   - Extraction didn't go far enough (still complex)
   - Naming could be clearer
   - Missing tests for extracted functions
   - Documentation needs updating

2. **Response:**
   - Address feedback promptly
   - Ask clarifying questions if needed
   - Push updates to same branch
   - Re-request review after changes

3. **Escalation:**
   - If fundamental disagreement on approach → discuss synchronously
   - If blocking on team decision → bring to team meeting
   - If reveals need for design doc → may need to pause and document

#### What if multiple PRs conflict?

1. **Prevention:**
   - Coordinate work assignment upfront
   - Use tracking document to see what's in flight
   - Communicate before starting work on a file

2. **Resolution if conflicts occur:**
   ```bash
   # Rebase on latest main
   git checkout main
   git pull origin main
   git checkout refactor/your-branch
   git rebase main

   # Resolve conflicts
   # Test after rebasing
   cargo nextest run

   # Force push (since history changed)
   git push --force-with-lease
   ```

3. **Coordination:**
   - Merge PRs in priority order (Phase 1 before Phase 2)
   - Communicate when PR is merged so others can rebase
   - Consider pausing work if major conflict expected

### Time Estimates: Important Caveats

The time estimates in this plan (e.g., "8-12 hours") should be interpreted as:

1. **Assumptions:**
   - Developer is familiar with Rust and the fastskill codebase
   - Developer has read CLAUDE.md, STYLE.md, and relevant documentation
   - Development environment is already set up
   - Time includes coding, testing, and initial PR iteration
   - Time does NOT include PR review wait time or significant rework

2. **Variables that affect time:**
   - **+50% time** if developer is new to codebase
   - **+30% time** if extensive new tests needed
   - **+20% time** if snapshot tests need significant updates
   - **+100% time** if deeper architectural issues discovered
   - **-20% time** if similar refactoring already done (pattern established)

3. **Recommendations:**
   - Use estimates for relative prioritization, not absolute planning
   - Track actual time spent to improve future estimates
   - Budget 20-30% buffer for unexpected issues
   - Plan for 1-2 PR review cycles adding 1-2 days each

### Developer Workflow Checklist

Use this checklist for each file refactoring:

#### Before Starting
- [ ] Read the relevant section of this plan thoroughly
- [ ] Verify you're assigned to this file (check tracking document)
- [ ] Ensure no one else is working on the same file
- [ ] Run `cargo nextest run --all-features` to establish baseline
- [ ] Check current line numbers match plan (code may have changed since analysis)
- [ ] Create feature branch: `refactor/<component>-<brief-description>`
- [ ] Verify CodeScene score matches plan (or document current score)
- [ ] Read existing tests for the file to understand behavior

#### During Refactoring
- [ ] Apply one refactoring pattern at a time (don't combine multiple changes)
- [ ] Write/update unit tests for extracted functions
- [ ] Run `cargo nextest run` after each logical change
- [ ] Commit incrementally with clear messages
- [ ] Run `cargo fmt --all && cargo clippy --workspace --all-targets --all-features`
- [ ] Update snapshots if needed: `cargo insta review` then `cargo insta accept`
- [ ] Verify behavior unchanged (run command manually if CLI, test API if HTTP)
- [ ] Keep notes on any unexpected issues or discoveries

#### Before Creating PR
- [ ] All tests pass: `cargo nextest run --all-features`
- [ ] No clippy warnings: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] Code formatted: `cargo fmt --all`
- [ ] No typos: `typos`
- [ ] No unused dependencies: `cargo shear`
- [ ] Snapshot tests accepted: `cargo insta test --test-runner nextest`
- [ ] Complexity reduced (verify with CodeScene or manual count)
- [ ] Code coverage maintained or improved (check if using coverage tools)
- [ ] Squash/rebase commits if needed for clean history
- [ ] Write PR description with before/after metrics

#### During PR Review
- [ ] Link to this plan in PR description
- [ ] Respond to review comments promptly
- [ ] Re-run tests after addressing feedback
- [ ] Update PR description if scope changed
- [ ] Re-request review after making changes
- [ ] Ensure CI passes (if automated CI configured)

#### After PR Merged
- [ ] Update tracking document to mark as completed
- [ ] Record actual time spent vs. estimate
- [ ] Document any lessons learned or unexpected issues
- [ ] Delete feature branch locally and remotely
- [ ] Update main branch: `git checkout main && git pull origin main`
- [ ] Celebrate the improvement! 🎉

## Conclusion

This plan provides a comprehensive approach to improving code health in the fastskill codebase. By systematically addressing each identified issue and applying proven refactoring patterns, we can achieve the target score of >8.0 for all files while maintaining functionality and improving maintainability.

The phased implementation approach ensures critical issues are addressed first while building momentum for smaller improvements. Each refactoring includes clear, actionable recommendations with expected outcomes.

**With the added Implementation Guide, developers now have:**
- Step-by-step workflow from branch creation to PR merge
- Clear success criteria and validation steps
- Coordination guidelines to prevent conflicts
- Troubleshooting guidance for common issues
- Realistic time estimates with caveats
- Comprehensive checklists for each phase

**Next Steps:**
1. Review this plan with the development team
2. Set up progress tracking (GitHub Project, spreadsheet, etc.)
3. Assign Phase 1 files to senior developers
4. Schedule kickoff meeting to align on approach
5. Begin refactoring work following the Implementation Guide
6. Track progress and adjust plan as needed based on learnings
