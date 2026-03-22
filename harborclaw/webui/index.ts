// HarborClaw WebUI — barrel exports for HarborOS integration.

// ---------------------------------------------------------------------------
// Settings page
// ---------------------------------------------------------------------------

// Interfaces
export {
  Channel,
  ChannelConfig,
  ChannelMeta,
  CHANNEL_META,
  Autonomy,
  AutonomyConfig,
  Route,
  RouteStatus,
  DEFAULT_ROUTE_PRIORITY,
  ConnectivityResult,
  HarborClawSettings,
} from './interfaces/harborclaw-settings.interface';

// Service
export { HarborClawSettingsService } from './services/harborclaw-settings.service';

// Components
export { HarborClawSettingsComponent } from './pages/harborclaw-settings/harborclaw-settings.component';
export { ChannelConfigComponent } from './pages/harborclaw-settings/components/channel-config/channel-config.component';
export { AutonomyConfigComponent } from './pages/harborclaw-settings/components/autonomy-config/autonomy-config.component';
export { RouteStrategyComponent } from './pages/harborclaw-settings/components/route-strategy/route-strategy.component';
export { ConnectivityTestComponent } from './pages/harborclaw-settings/components/connectivity-test/connectivity-test.component';

// Route elements
export { harborClawSettingsElements } from './pages/harborclaw-settings/harborclaw-settings.elements';

// ---------------------------------------------------------------------------
// Extension Manager page
// ---------------------------------------------------------------------------

// Interfaces
export {
  ExtensionType,
  ExtensionTypeMeta,
  EXTENSION_TYPE_META,
  RiskLevel,
  ExtensionSummary,
  ExtensionDetail,
  ExecutorConfig,
  HarborApiConfig,
  HarborCliConfig,
  RiskConfig,
  ValidationResult,
  ExtensionFilter,
} from './interfaces/extension.interface';

// Service
export { ExtensionService } from './services/extension.service';

// Components
export { ExtensionManagerComponent } from './pages/extension-manager/extension-manager.component';
export { ExtensionListComponent } from './pages/extension-manager/components/extension-list/extension-list.component';
export { ExtensionDetailComponent } from './pages/extension-manager/components/extension-detail/extension-detail.component';
export { ExtensionImportComponent } from './pages/extension-manager/components/extension-import/extension-import.component';

// Route elements
export { extensionManagerElements } from './pages/extension-manager/extension-manager.elements';
