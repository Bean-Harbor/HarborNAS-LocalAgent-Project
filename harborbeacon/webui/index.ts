// HarborBeacon WebUI — barrel exports for HarborOS integration.

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
  HarborBeaconSettings,
} from './interfaces/harborbeacon-settings.interface';

// Service
export { HarborBeaconSettingsService } from './services/harborbeacon-settings.service';

// Components
export { HarborBeaconSettingsComponent } from './pages/harborbeacon-settings/harborbeacon-settings.component';
export { ChannelConfigComponent } from './pages/harborbeacon-settings/components/channel-config/channel-config.component';
export { AutonomyConfigComponent } from './pages/harborbeacon-settings/components/autonomy-config/autonomy-config.component';
export { RouteStrategyComponent } from './pages/harborbeacon-settings/components/route-strategy/route-strategy.component';
export { ConnectivityTestComponent } from './pages/harborbeacon-settings/components/connectivity-test/connectivity-test.component';

// Route elements
export { harborBeaconSettingsElements } from './pages/harborbeacon-settings/harborbeacon-settings.elements';

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
