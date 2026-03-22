/**
 * HarborClaw Extension Manager — TypeScript interfaces.
 *
 * Mirror the Python dataclasses in harborclaw/api/extensions_api.py.
 * Designed for extensibility: `ExtensionType` can be widened as new
 * kinds of extensions are added (workflow, integration, automation …).
 */

// ---------------------------------------------------------------------------
// Extension type — extensible discriminator
// ---------------------------------------------------------------------------

export enum ExtensionType {
  Skill = 'skill',
  Workflow = 'workflow',
  Integration = 'integration',
  Automation = 'automation',
}

export interface ExtensionTypeMeta {
  type: ExtensionType;
  label: string;
  icon: string;        // Material icon name
  color: string;       // CSS class / badge colour token
}

export const EXTENSION_TYPE_META: ExtensionTypeMeta[] = [
  { type: ExtensionType.Skill, label: 'Skill', icon: 'psychology', color: 'primary' },
  { type: ExtensionType.Workflow, label: 'Workflow', icon: 'account_tree', color: 'accent' },
  { type: ExtensionType.Integration, label: 'Integration', icon: 'hub', color: 'warn' },
  { type: ExtensionType.Automation, label: 'Automation', icon: 'schedule', color: '' },
];

// ---------------------------------------------------------------------------
// Risk
// ---------------------------------------------------------------------------

export enum RiskLevel {
  LOW = 'LOW',
  MEDIUM = 'MEDIUM',
  HIGH = 'HIGH',
  CRITICAL = 'CRITICAL',
}

// ---------------------------------------------------------------------------
// Extension summary (list view card)
// ---------------------------------------------------------------------------

export interface ExtensionSummary {
  id: string;
  name: string;
  type: ExtensionType;
  version: string;
  summary: string;
  owner: string;
  capabilities: string[];
  risk_level: RiskLevel;
  enabled: boolean;
}

// ---------------------------------------------------------------------------
// Executor & route configs (detail view)
// ---------------------------------------------------------------------------

export interface ExecutorConfig {
  enabled: boolean;
  command?: string | null;
}

export interface HarborApiConfig {
  enabled: boolean;
  provider: string;
  endpoint_group: string;
  allowed_methods: string[];
  min_version: string;
}

export interface HarborCliConfig {
  enabled: boolean;
  tool: string;
  command_group: string;
  allowed_subcommands: string[];
  require_structured_output: boolean;
}

export interface RiskConfig {
  default_level: RiskLevel;
  requires_confirmation: string[];
}

// ---------------------------------------------------------------------------
// Full extension detail (edit view)
// ---------------------------------------------------------------------------

export interface ExtensionDetail {
  id: string;
  name: string;
  type: ExtensionType;
  version: string;
  summary: string;
  owner: string;
  capabilities: string[];
  executors: Record<string, ExecutorConfig>;
  harbor_api: HarborApiConfig;
  harbor_cli: HarborCliConfig;
  permissions: Record<string, unknown>;
  risk: RiskConfig;
  input_schema: Record<string, unknown>;
  output_schema: Record<string, unknown>;
  enabled: boolean;
}

// ---------------------------------------------------------------------------
// Validation result (import / edit feedback)
// ---------------------------------------------------------------------------

export interface ValidationResult {
  valid: boolean;
  extension_id?: string | null;
  extension_name?: string | null;
  errors: string[];
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Filter state (list page)
// ---------------------------------------------------------------------------

export interface ExtensionFilter {
  search: string;
  types: ExtensionType[];
  riskLevels: RiskLevel[];
  enabledOnly: boolean;
}
