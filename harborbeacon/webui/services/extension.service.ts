import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';

import {
  ExtensionSummary,
  ExtensionDetail,
  ValidationResult,
} from '../interfaces/extension.interface';

/**
 * Angular service for the HarborBeacon Extension Manager.
 *
 * Endpoints mirror `harborbeacon.api.extensions_api`:
 *   GET    /api/v2.0/harborbeacon/extensions
 *   GET    /api/v2.0/harborbeacon/extensions/:id
 *   POST   /api/v2.0/harborbeacon/extensions
 *   PUT    /api/v2.0/harborbeacon/extensions/:id
 *   DELETE /api/v2.0/harborbeacon/extensions/:id
 *   POST   /api/v2.0/harborbeacon/extensions/validate
 */
@Injectable({ providedIn: 'root' })
export class ExtensionService {
  private readonly base = '/api/v2.0/harborbeacon/extensions';

  constructor(private http: HttpClient) {}

  // ---- List / Detail ----

  listExtensions(): Observable<ExtensionSummary[]> {
    return this.http.get<ExtensionSummary[]>(this.base);
  }

  getExtension(id: string): Observable<ExtensionDetail> {
    return this.http.get<ExtensionDetail>(`${this.base}/${encodeURIComponent(id)}`);
  }

  // ---- Mutations ----

  createExtension(body: Record<string, unknown>): Observable<ExtensionDetail> {
    return this.http.post<ExtensionDetail>(this.base, body);
  }

  updateExtension(id: string, body: Record<string, unknown>): Observable<ExtensionDetail> {
    return this.http.put<ExtensionDetail>(
      `${this.base}/${encodeURIComponent(id)}`,
      body,
    );
  }

  deleteExtension(id: string): Observable<void> {
    return this.http.delete<void>(`${this.base}/${encodeURIComponent(id)}`);
  }

  // ---- Validation ----

  validateExtension(body: Record<string, unknown>): Observable<ValidationResult> {
    return this.http.post<ValidationResult>(`${this.base}/validate`, body);
  }
}
