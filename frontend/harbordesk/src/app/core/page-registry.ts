export type HarborDeskPageId =
  | 'overview'
  | 'im-gateway'
  | 'account-management'
  | 'tasks-approvals'
  | 'devices-aiot'
  | 'harboros'
  | 'models-policies'
  | 'system-settings';

export interface HarborDeskPageDefinition {
  id: HarborDeskPageId;
  path: string;
  label: string;
  tagline: string;
  accent: 'teal' | 'amber' | 'sky' | 'rose';
}

export const HARBORDESK_PAGES: readonly HarborDeskPageDefinition[] = [
  { id: 'overview', path: 'overview', label: 'Overview', tagline: 'Command surface and status digest', accent: 'teal' },
  { id: 'im-gateway', path: 'im-gateway', label: 'IM Gateway', tagline: 'Bridge state and source-bound delivery', accent: 'sky' },
  { id: 'account-management', path: 'account-management', label: 'Account Management', tagline: 'Members, roles, and binding availability', accent: 'amber' },
  { id: 'tasks-approvals', path: 'tasks-approvals', label: 'Tasks & Approvals', tagline: 'High-risk actions and audited review', accent: 'rose' },
  { id: 'devices-aiot', path: 'devices-aiot', label: 'Devices & AIoT', tagline: 'Discovery, preview, and device governance', accent: 'teal' },
  { id: 'harboros', path: 'harboros', label: 'HarborOS', tagline: 'System-domain boundaries and live/proof split', accent: 'sky' },
  { id: 'models-policies', path: 'models-policies', label: 'Models & Policies', tagline: 'Endpoint status, policy, and fallback order', accent: 'amber' },
  { id: 'system-settings', path: 'system-settings', label: 'System Settings', tagline: 'Routing, gateway status, and blockers', accent: 'rose' }
] as const;

export function pageById(pageId: HarborDeskPageId): HarborDeskPageDefinition {
  const page = HARBORDESK_PAGES.find((candidate) => candidate.id === pageId);
  if (!page) {
    throw new Error(`Unknown HarborDesk page id: ${pageId}`);
  }
  return page;
}
