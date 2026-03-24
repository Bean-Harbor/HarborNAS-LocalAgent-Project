import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';

import {
  Channel,
  ChannelConfig,
  ConnectivityResult,
  FeishuBrowserSetupResumeRequest,
  FeishuBrowserSetupSession,
  FeishuBrowserSetupStartRequest,
  FeishuOneClickSetupRequest,
  FeishuOneClickSetupResult,
  HarborBeaconSettings,
  RouteStatus,
} from '../interfaces/harborbeacon-settings.interface';

/**
 * Angular service that talks to the HarborOS middleware API
 * for HarborBeacon settings CRUD.
 *
 * Endpoints mirror `harborbeacon.api.settings_api`:
 *   GET  /api/v2.0/harborbeacon/settings
 *   PUT  /api/v2.0/harborbeacon/settings
 *   POST /api/v2.0/harborbeacon/settings/test_channel
 *   POST /api/v2.0/harborbeacon/settings/feishu/one_click_setup
 *   GET  /api/v2.0/harborbeacon/routes/status
 */
@Injectable({ providedIn: 'root' })
export class HarborBeaconSettingsService {
  private readonly base = '/api/v2.0/harborbeacon';

  constructor(private http: HttpClient) {}

  // ---- Settings CRUD ----

  getSettings(): Observable<HarborBeaconSettings> {
    return this.http.get<HarborBeaconSettings>(`${this.base}/settings`);
  }

  saveSettings(settings: HarborBeaconSettings): Observable<HarborBeaconSettings> {
    return this.http.put<HarborBeaconSettings>(`${this.base}/settings`, settings);
  }

  // ---- Channel connectivity ----

  testChannel(channel: Channel, config: ChannelConfig): Observable<ConnectivityResult> {
    return this.http.post<ConnectivityResult>(
      `${this.base}/settings/test_channel`,
      { channel: channel, config },
    );
  }

  testAllChannels(): Observable<ConnectivityResult[]> {
    return this.http.post<ConnectivityResult[]>(
      `${this.base}/settings/test_channels`,
      {},
    );
  }

  oneClickSetupFeishu(
    payload: FeishuOneClickSetupRequest,
  ): Observable<FeishuOneClickSetupResult> {
    return this.http.post<FeishuOneClickSetupResult>(
      `${this.base}/settings/feishu/one_click_setup`,
      payload,
    );
  }

  // ---- Feishu browser-assisted setup ----

  browserSetupFeishuStart(
    payload: FeishuBrowserSetupStartRequest,
  ): Observable<FeishuBrowserSetupSession> {
    return this.http.post<FeishuBrowserSetupSession>(
      `${this.base}/settings/feishu/browser_setup/start`,
      payload,
    );
  }

  browserSetupFeishuResume(
    payload: FeishuBrowserSetupResumeRequest,
  ): Observable<FeishuBrowserSetupSession> {
    return this.http.post<FeishuBrowserSetupSession>(
      `${this.base}/settings/feishu/browser_setup/resume`,
      payload,
    );
  }

  browserSetupFeishuStatus(
    sessionId: string,
  ): Observable<FeishuBrowserSetupSession> {
    return this.http.get<FeishuBrowserSetupSession>(
      `${this.base}/settings/feishu/browser_setup/status`,
      { params: { session_id: sessionId } },
    );
  }

  // ---- Route status ----

  getRouteStatus(): Observable<RouteStatus[]> {
    return this.http.get<RouteStatus[]>(`${this.base}/routes/status`);
  }
}
