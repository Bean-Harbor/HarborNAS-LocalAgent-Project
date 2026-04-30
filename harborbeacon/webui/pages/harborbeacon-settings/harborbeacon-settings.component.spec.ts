/**
 * Jest unit tests for HarborBeaconSettingsComponent.
 *
 * These tests verify the container component's data flow, tab
 * structure, save/load lifecycle, and child component wiring.
 *
 * Prerequisites (in the HarborOS WebUI project):
 *   - Angular TestBed
 *   - jest-preset-angular
 *   - HttpClientTestingModule
 */
import { ComponentFixture, TestBed, fakeAsync, tick } from '@angular/core/testing';
import { HttpClientTestingModule, HttpTestingController } from '@angular/common/http/testing';
import { NoopAnimationsModule } from '@angular/platform-browser/animations';
import { MatSnackBarModule } from '@angular/material/snack-bar';

import { HarborBeaconSettingsComponent } from './harborbeacon-settings.component';
import {
  Autonomy,
  Channel,
  HarborBeaconSettings,
  Route,
} from '../../interfaces/harborbeacon-settings.interface';

const MOCK_SETTINGS: HarborBeaconSettings = {
  channels: [
    { channel: Channel.Feishu, enabled: true, app_id: 'cli_xxx', app_secret: 'sec', extra: {} },
    { channel: Channel.Telegram, enabled: false, extra: {} },
  ],
  autonomy: {
    default_level: Autonomy.Supervised,
    approval_timeout_seconds: 120,
    allow_full_for_channels: [],
  },
  route_priority: [Route.MiddlewareApi, Route.Midcli, Route.Browser, Route.Mcp],
};

describe('HarborBeaconSettingsComponent', () => {
  let fixture: ComponentFixture<HarborBeaconSettingsComponent>;
  let component: HarborBeaconSettingsComponent;
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [
        HarborBeaconSettingsComponent,
        HttpClientTestingModule,
        NoopAnimationsModule,
        MatSnackBarModule,
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(HarborBeaconSettingsComponent);
    component = fixture.componentInstance;
    httpMock = TestBed.inject(HttpTestingController);
  });

  afterEach(() => {
    httpMock.verify();
  });

  function flushInitialLoad(): void {
    // ngOnInit triggers GET /settings + GET /routes/status
    const settingsReq = httpMock.expectOne('/api/v2.0/harborbeacon/settings');
    settingsReq.flush(MOCK_SETTINGS);

    const routesReq = httpMock.expectOne('/api/v2.0/harborbeacon/routes/status');
    routesReq.flush([
      { route: 'middleware_api', label: 'Middleware API', available: true, priority: 1 },
      { route: 'midcli', label: 'midcli', available: true, priority: 2 },
      { route: 'browser', label: 'Browser', available: false, priority: 3 },
      { route: 'mcp', label: 'MCP', available: true, priority: 4 },
    ]);
  }

  it('should load settings on init', () => {
    fixture.detectChanges(); // triggers ngOnInit
    flushInitialLoad();
    fixture.detectChanges();

    expect(component.loading).toBe(false);
    expect(component.channels.length).toBe(2);
    expect(component.autonomy.default_level).toBe(Autonomy.Supervised);
  });

  it('should set dirty when channels change', () => {
    fixture.detectChanges();
    flushInitialLoad();

    expect(component.dirty).toBe(false);
    component.onChannelsChange([
      { channel: Channel.Feishu, enabled: false, extra: {} },
    ]);
    expect(component.dirty).toBe(true);
  });

  it('should set dirty when autonomy changes', () => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onAutonomyChange({
      default_level: Autonomy.Full,
      approval_timeout_seconds: 60,
      allow_full_for_channels: [],
    });
    expect(component.dirty).toBe(true);
  });

  it('should set dirty when route priority changes', () => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onRoutePriorityChange([Route.Mcp, Route.Browser, Route.Midcli, Route.MiddlewareApi]);
    expect(component.dirty).toBe(true);
    expect(component.routePriority[0]).toBe(Route.Mcp);
  });

  it('should save settings via PUT', fakeAsync(() => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onAutonomyChange({
      ...component.autonomy,
      default_level: Autonomy.Full,
    });
    component.save();

    const req = httpMock.expectOne({ method: 'PUT', url: '/api/v2.0/harborbeacon/settings' });
    expect(req.request.body.autonomy.default_level).toBe('Full');

    req.flush({ ...MOCK_SETTINGS, autonomy: { ...MOCK_SETTINGS.autonomy, default_level: Autonomy.Full } });
    tick();

    expect(component.dirty).toBe(false);
    expect(component.saving).toBe(false);
  }));

  it('should test a single channel', fakeAsync(() => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onTestChannel({
      channel: Channel.Feishu,
      config: { channel: Channel.Feishu, enabled: true, app_id: 'x', app_secret: 'y', extra: {} },
    });

    expect(component.testingConnectivity).toBe(true);

    const req = httpMock.expectOne('/api/v2.0/harborbeacon/settings/test_channel');
    req.flush({
      channel: 'feishu',
      reachable: true,
      latency_ms: 42,
      tested_at: '2026-03-22T10:00:00Z',
    });
    tick();

    expect(component.testingConnectivity).toBe(false);
    expect(component.connectivityResults.length).toBe(1);
    expect(component.connectivityResults[0].reachable).toBe(true);
  }));

  it('should test all channels', fakeAsync(() => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onRunConnectivityTest(null);

    const req = httpMock.expectOne('/api/v2.0/harborbeacon/settings/test_channels');
    req.flush([
      { channel: 'feishu', reachable: true, latency_ms: 30, tested_at: '2026-03-22T10:00:00Z' },
    ]);
    tick();

    expect(component.connectivityResults.length).toBe(1);
  }));

  it('should validate and apply Feishu config', fakeAsync(() => {
    fixture.detectChanges();
    flushInitialLoad();

    component.onApplyFeishuConfig({
      channel: Channel.Feishu,
      enabled: true,
      app_id: 'cli_xxx',
      app_secret: 'sec_xxx',
      extra: {},
    });

    expect(component.feishuSetupRunning).toBe(true);

    const req = httpMock.expectOne('/api/v2.0/harborbeacon/settings/feishu/configure');
    req.flush({
      success: true,
      message: 'ok',
      settings_updated: true,
      connectivity: {
        channel: 'feishu',
        reachable: true,
        latency_ms: 12,
        tested_at: '2026-03-24T10:00:00Z',
      },
      bot_info: {},
      next_steps: [],
      settings: MOCK_SETTINGS,
    });
    tick();

    expect(component.feishuSetupRunning).toBe(false);
    expect(component.feishuSetupResult?.success).toBe(true);
    expect(component.channels.length).toBe(2);
  }));

  it('should have 4 route statuses after load', () => {
    fixture.detectChanges();
    flushInitialLoad();
    fixture.detectChanges();

    expect(component.routeStatuses.length).toBe(4);
    expect(component.routeStatuses[2].available).toBe(false); // browser offline
  });
});
