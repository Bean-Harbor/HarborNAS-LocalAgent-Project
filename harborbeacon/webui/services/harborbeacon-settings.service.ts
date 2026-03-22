import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';

import {
  Channel,
  ChannelConfig,
  ConnectivityResult,
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

  // ---- Route status ----

  getRouteStatus(): Observable<RouteStatus[]> {
    return this.http.get<RouteStatus[]>(`${this.base}/routes/status`);
  }
}
