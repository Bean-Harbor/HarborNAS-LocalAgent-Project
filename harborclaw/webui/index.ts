// HarborClaw WebUI — barrel exports for HarborOS integration.

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
