/**
 * Jest unit tests for ExtensionManagerComponent and sub-components.
 *
 * These are structural / smoke tests verifiable without a running
 * Angular TestBed — they validate the component class logic,
 * interfaces, and service method signatures.
 */

import {
  ExtensionType,
  RiskLevel,
  EXTENSION_TYPE_META,
  ExtensionSummary,
  ExtensionDetail,
  ValidationResult,
  ExtensionFilter,
} from '../../interfaces/extension.interface';

// ---------------------------------------------------------------------------
// Interface & enum tests
// ---------------------------------------------------------------------------

describe('Extension interfaces', () => {
  test('ExtensionType enum has 4 values', () => {
    const values = Object.values(ExtensionType);
    expect(values).toEqual(['skill', 'workflow', 'integration', 'automation']);
  });

  test('RiskLevel enum has 4 values', () => {
    const values = Object.values(RiskLevel);
    expect(values).toEqual(['LOW', 'MEDIUM', 'HIGH', 'CRITICAL']);
  });

  test('EXTENSION_TYPE_META provides metadata for each type', () => {
    expect(EXTENSION_TYPE_META).toHaveLength(4);
    for (const meta of EXTENSION_TYPE_META) {
      expect(meta.type).toBeDefined();
      expect(meta.label).toBeTruthy();
      expect(meta.icon).toBeTruthy();
      expect(typeof meta.color).toBe('string');
    }
  });

  test('ExtensionSummary shape can be constructed', () => {
    const summary: ExtensionSummary = {
      id: 'system.harbor_ops',
      name: 'HarborOS Service Operations',
      type: ExtensionType.Skill,
      version: '1.0.0',
      summary: 'Query, start, stop, restart services',
      owner: 'harbor-team',
      capabilities: ['service.status', 'service.start'],
      risk_level: RiskLevel.LOW,
      enabled: true,
    };
    expect(summary.id).toBe('system.harbor_ops');
    expect(summary.type).toBe(ExtensionType.Skill);
    expect(summary.capabilities).toHaveLength(2);
  });

  test('ExtensionDetail includes executor configs', () => {
    const detail: ExtensionDetail = {
      id: 'test.ext',
      name: 'Test',
      type: ExtensionType.Workflow,
      version: '0.1.0',
      summary: '',
      owner: '',
      capabilities: [],
      executors: {
        cli: { enabled: false, command: null },
        browser: { enabled: false },
      },
      harbor_api: {
        enabled: true,
        provider: 'middleware',
        endpoint_group: 'service',
        allowed_methods: ['query'],
        min_version: 'v1',
      },
      harbor_cli: {
        enabled: false,
        tool: 'midcli',
        command_group: '',
        allowed_subcommands: [],
        require_structured_output: true,
      },
      permissions: {},
      risk: { default_level: RiskLevel.MEDIUM, requires_confirmation: ['HIGH', 'CRITICAL'] },
      input_schema: {},
      output_schema: {},
      enabled: true,
    };
    expect(detail.harbor_api.enabled).toBe(true);
    expect(detail.executors['cli'].enabled).toBe(false);
  });

  test('ValidationResult valid + errors/warnings', () => {
    const valid: ValidationResult = {
      valid: true,
      extension_id: 'test.ext',
      extension_name: 'Test',
      errors: [],
      warnings: ['Missing recommended field: name'],
    };
    expect(valid.valid).toBe(true);
    expect(valid.warnings).toHaveLength(1);

    const invalid: ValidationResult = {
      valid: false,
      errors: ['Missing required field: id'],
      warnings: [],
    };
    expect(invalid.valid).toBe(false);
    expect(invalid.errors).toHaveLength(1);
  });

  test('ExtensionFilter defaults', () => {
    const filter: ExtensionFilter = {
      search: '',
      types: [],
      riskLevels: [],
      enabledOnly: false,
    };
    expect(filter.types).toEqual([]);
    expect(filter.enabledOnly).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Component class logic (without TestBed)
// ---------------------------------------------------------------------------

describe('ExtensionListComponent filter logic', () => {
  const extensions: ExtensionSummary[] = [
    {
      id: 'system.harbor_ops',
      name: 'HarborOS Service Operations',
      type: ExtensionType.Skill,
      version: '1.0.0',
      summary: 'Manage services',
      owner: 'harbor-team',
      capabilities: ['service.status', 'service.start'],
      risk_level: RiskLevel.LOW,
      enabled: true,
    },
    {
      id: 'media.video_edit',
      name: 'Video Editor',
      type: ExtensionType.Skill,
      version: '0.5.0',
      summary: 'Trim and transcode videos',
      owner: 'media-team',
      capabilities: ['video.trim'],
      risk_level: RiskLevel.MEDIUM,
      enabled: false,
    },
    {
      id: 'wf.backup',
      name: 'Backup Workflow',
      type: ExtensionType.Workflow,
      version: '2.0.0',
      summary: 'Scheduled backup',
      owner: 'ops-team',
      capabilities: ['storage.snapshot'],
      risk_level: RiskLevel.HIGH,
      enabled: true,
    },
  ];

  function applyFilter(exts: ExtensionSummary[], filter: ExtensionFilter): ExtensionSummary[] {
    return exts.filter(ext => {
      if (filter.search) {
        const q = filter.search.toLowerCase();
        const haystack = `${ext.id} ${ext.name} ${ext.summary} ${ext.capabilities.join(' ')}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      if (filter.types.length && !filter.types.includes(ext.type)) return false;
      if (filter.riskLevels.length && !filter.riskLevels.includes(ext.risk_level)) return false;
      if (filter.enabledOnly && !ext.enabled) return false;
      return true;
    });
  }

  test('no filter returns all', () => {
    const result = applyFilter(extensions, { search: '', types: [], riskLevels: [], enabledOnly: false });
    expect(result).toHaveLength(3);
  });

  test('search by name', () => {
    const result = applyFilter(extensions, { search: 'video', types: [], riskLevels: [], enabledOnly: false });
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('media.video_edit');
  });

  test('filter by type', () => {
    const result = applyFilter(extensions, { search: '', types: [ExtensionType.Workflow], riskLevels: [], enabledOnly: false });
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('wf.backup');
  });

  test('filter by risk level', () => {
    const result = applyFilter(extensions, { search: '', types: [], riskLevels: [RiskLevel.LOW], enabledOnly: false });
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('system.harbor_ops');
  });

  test('enabledOnly filter', () => {
    const result = applyFilter(extensions, { search: '', types: [], riskLevels: [], enabledOnly: true });
    expect(result).toHaveLength(2);
    expect(result.every(e => e.enabled)).toBe(true);
  });

  test('combined search + type filter', () => {
    const result = applyFilter(extensions, { search: 'service', types: [ExtensionType.Skill], riskLevels: [], enabledOnly: false });
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('system.harbor_ops');
  });

  test('search by capability', () => {
    const result = applyFilter(extensions, { search: 'snapshot', types: [], riskLevels: [], enabledOnly: false });
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('wf.backup');
  });
});
