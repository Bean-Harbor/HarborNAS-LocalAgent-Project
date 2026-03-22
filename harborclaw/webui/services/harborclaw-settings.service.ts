import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';

import {
  Channel,
  ChannelConfig,
  ConnectivityResult,
  HarborClawSettings,
  RouteStatus,
} from '../interfaces/harborclaw-settings.interface';

/**
 * Angular service that talks to the HarborOS middleware API
 * for HarborClaw settings CRUD.
 *
 * Endpoints mirror `harborclaw.api.settings_api`:
 *   GET  /api/v2.0/harborclaw/settings
 *   PUT  /api/v2.0/harborclaw/settings
 *   POST /api/v2.0/harborclaw/settings/test_channel
 *   GET  /api/v2.0/harborclaw/routes/status
 */
@Injectable({ providedIn: 'root' })
export class HarborClawSettingsService {
  private readonly base = '/api/v2.0/harborclaw';

  constructor(private http: HttpClient) {}

  // ---- Settings CRUD ----

  getSettings(): Observable<HarborClawSettings> {
    return this.http.get<HarborClawSettings>(`${this.base}/settings`);
  }

  saveSettings(settings: HarborClawSettings): Observable<HarborClawSettings> {
    return this.http.put<HarborClawSettings>(`${this.base}/settings`, settings);
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
