import {
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  OnInit,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { MatTabsModule } from '@angular/material/tabs';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { MatProgressBarModule } from '@angular/material/progress-bar';

import {
  Autonomy,
  AutonomyConfig,
  Channel,
  ChannelConfig,
  ConnectivityResult,
  FeishuConfigApplyResult,
  HarborBeaconSettings,
  Route,
  RouteStatus,
  DEFAULT_ROUTE_PRIORITY,
} from '../../interfaces/harborbeacon-settings.interface';

import { HarborBeaconSettingsService } from '../../services/harborbeacon-settings.service';
import { ChannelConfigComponent } from './components/channel-config/channel-config.component';
import { AutonomyConfigComponent } from './components/autonomy-config/autonomy-config.component';
import { RouteStrategyComponent } from './components/route-strategy/route-strategy.component';
import { ConnectivityTestComponent } from './components/connectivity-test/connectivity-test.component';

@Component({
  selector: 'ix-harborbeacon-settings',
  standalone: true,
  imports: [
    CommonModule,
    MatTabsModule,
    MatIconModule,
    MatButtonModule,
    MatSnackBarModule,
    MatProgressBarModule,
    ChannelConfigComponent,
    AutonomyConfigComponent,
    RouteStrategyComponent,
    ConnectivityTestComponent,
  ],
  templateUrl: './harborbeacon-settings.component.html',
  styleUrls: ['./harborbeacon-settings.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HarborBeaconSettingsComponent implements OnInit {
  // ---- state ----
  channels: ChannelConfig[] = [];
  autonomy: AutonomyConfig = {
    default_level: Autonomy.Supervised,
    approval_timeout_seconds: 120,
    allow_full_for_channels: [],
  };
  routePriority: Route[] = [...DEFAULT_ROUTE_PRIORITY];
  routeStatuses: RouteStatus[] = [];
  connectivityResults: ConnectivityResult[] = [];
  testingConnectivity = false;
  feishuSetupRunning = false;
  feishuSetupResult: FeishuConfigApplyResult | null = null;

  saving = false;
  loading = true;
  dirty = false;

  constructor(
    private settingsService: HarborBeaconSettingsService,
    private snackBar: MatSnackBar,
    private cdr: ChangeDetectorRef,
  ) {}

  ngOnInit(): void {
    this.load();
  }

  // ---- data flow ----

  onChannelsChange(channels: ChannelConfig[]): void {
    this.channels = channels;
    this.dirty = true;
  }

  onAutonomyChange(config: AutonomyConfig): void {
    this.autonomy = config;
    this.dirty = true;
  }

  onRoutePriorityChange(priority: Route[]): void {
    this.routePriority = priority;
    this.dirty = true;
  }

  onTestChannel(event: { channel: Channel; config: ChannelConfig }): void {
    this.testingConnectivity = true;
    this.cdr.markForCheck();
    this.settingsService.testChannel(event.channel, event.config).subscribe({
      next: (result) => {
        this.mergeConnectivityResult(result);
        this.testingConnectivity = false;
        this.cdr.markForCheck();
      },
      error: () => {
        this.snackBar.open('Connection test failed', 'OK', { duration: 4000 });
        this.testingConnectivity = false;
        this.cdr.markForCheck();
      },
    });
  }

  onRunConnectivityTest(channel: Channel | null): void {
    this.testingConnectivity = true;
    this.cdr.markForCheck();

    if (channel) {
      const config = this.channels.find((c) => c.channel === channel);
      if (config) {
        this.settingsService.testChannel(channel, config).subscribe({
          next: (result) => {
            this.mergeConnectivityResult(result);
            this.testingConnectivity = false;
            this.cdr.markForCheck();
          },
          error: () => {
            this.testingConnectivity = false;
            this.cdr.markForCheck();
          },
        });
      }
    } else {
      this.settingsService.testAllChannels().subscribe({
        next: (results) => {
          this.connectivityResults = results;
          this.testingConnectivity = false;
          this.cdr.markForCheck();
        },
        error: () => {
          this.snackBar.open('Batch connectivity test failed', 'OK', { duration: 4000 });
          this.testingConnectivity = false;
          this.cdr.markForCheck();
        },
      });
    }
  }

  onApplyFeishuConfig(config: ChannelConfig): void {
    this.feishuSetupRunning = true;
    this.cdr.markForCheck();
    this.settingsService.configureFeishu({
      app_id: config.app_id ?? '',
      app_secret: config.app_secret ?? '',
      webhook_url: config.webhook_url,
    }).subscribe({
      next: (result) => {
        this.feishuSetupResult = result;
        if (result.settings) {
          this.applySettings(result.settings);
          this.dirty = false;
        }
        if (result.connectivity) {
          this.mergeConnectivityResult(result.connectivity);
        }
        this.feishuSetupRunning = false;
        this.snackBar.open(result.success ? 'Feishu config validated and applied' : result.message, 'OK', {
          duration: result.success ? 3500 : 5000,
        });
        this.cdr.markForCheck();
      },
      error: () => {
        this.feishuSetupRunning = false;
        this.snackBar.open('Feishu config validation failed', 'OK', { duration: 5000 });
        this.cdr.markForCheck();
      },
    });
  }

  // ---- save / load ----

  save(): void {
    this.saving = true;
    this.cdr.markForCheck();

    const payload: HarborBeaconSettings = {
      channels: this.channels,
      autonomy: this.autonomy,
      route_priority: this.routePriority,
    };

    this.settingsService.saveSettings(payload).subscribe({
      next: (saved) => {
        this.applySettings(saved);
        this.dirty = false;
        this.saving = false;
        this.snackBar.open('Settings saved', '', { duration: 3000 });
        this.cdr.markForCheck();
      },
      error: () => {
        this.saving = false;
        this.snackBar.open('Failed to save settings', 'Retry', { duration: 5000 });
        this.cdr.markForCheck();
      },
    });
  }

  // ---- private ----

  private load(): void {
    this.loading = true;
    this.settingsService.getSettings().subscribe({
      next: (settings) => {
        this.applySettings(settings);
        this.loading = false;
        this.cdr.markForCheck();
      },
      error: () => {
        this.loading = false;
        this.cdr.markForCheck();
      },
    });

    this.settingsService.getRouteStatus().subscribe({
      next: (statuses) => {
        this.routeStatuses = statuses;
        this.cdr.markForCheck();
      },
    });
  }

  private applySettings(s: HarborBeaconSettings): void {
    this.channels = s.channels;
    this.autonomy = s.autonomy;
    this.routePriority = s.route_priority;
  }

  private mergeConnectivityResult(result: ConnectivityResult): void {
    const idx = this.connectivityResults.findIndex(
      (r) => r.channel === result.channel,
    );
    if (idx >= 0) {
      this.connectivityResults = [
        ...this.connectivityResults.slice(0, idx),
        result,
        ...this.connectivityResults.slice(idx + 1),
      ];
    } else {
      this.connectivityResults = [...this.connectivityResults, result];
    }
  }
}
