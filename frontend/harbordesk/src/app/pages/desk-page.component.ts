import { AsyncPipe } from '@angular/common';
import { Component, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { BehaviorSubject, Observable, combineLatest } from 'rxjs';
import { finalize, switchMap, tap } from 'rxjs/operators';

import {
  DeviceCredentialsPayload,
  DeliverySurface,
  DiscoveryScanPayload,
  FilesBrowseResponse,
  KnowledgeIndexRunResponse,
  KnowledgeSettings,
  LocalModelCatalogItem,
  ManualDevicePayload,
  ModelEndpointTestResult,
  RtspCheckPayload,
  RtspCheckResult,
  StartLocalModelDownloadRequest
} from '../core/admin-api.types';
import { HarborDeskAdminApiService } from '../core/admin-api.service';
import { HarborDeskPageId } from '../core/page-registry';
import { uiText } from '../core/ui-locale';
import { PageStatePanelComponent } from '../shared/page-state-panel.component';

@Component({
  standalone: true,
  imports: [AsyncPipe, PageStatePanelComponent],
  template: `
    <hd-page-state-panel
      [state]="state$ | async"
      [savingMemberId]="savingMemberId"
      [saveError]="saveError"
      [saveSuccess]="saveSuccess"
      [testingEndpointId]="testingEndpointId"
      [endpointTestResults]="endpointTestResults"
      [savingTargetId]="savingTargetId"
      [deletingTargetId]="deletingTargetId"
      [deviceActionBusyKey]="deviceActionBusyKey"
      [deviceActionResults]="deviceActionResults"
      [rtspCheckResults]="rtspCheckResults"
      [releaseReadinessBusy]="releaseReadinessBusy"
      [knowledgeIndexBusy]="knowledgeIndexBusy"
      [knowledgeIndexJobBusyId]="knowledgeIndexJobBusyId"
      [modelDownloadBusyId]="modelDownloadBusyId"
      [filesBrowse]="filesBrowse"
      (defaultDeliverySurfaceChange)="updateDefaultDeliverySurface($event.userId, $event.surface)"
      (notificationTargetDefaultChange)="setDefaultNotificationTarget($event)"
      (notificationTargetDelete)="deleteNotificationTarget($event)"
      (endpointTestRequested)="runEndpointTest($event)"
      (deviceScanRequested)="scanDevices($event)"
      (manualDeviceAddRequested)="addManualDevice($event)"
      (defaultCameraChange)="setDefaultCamera($event)"
      (deviceCredentialsSave)="saveDeviceCredentials($event.deviceId, $event.payload)"
      (deviceRtspCheck)="checkDeviceRtsp($event.deviceId, $event.payload)"
      (cameraSnapshotRequested)="testCameraSnapshot($event)"
      (cameraShareLinkCreate)="createCameraShareLink($event)"
      (deviceValidationRun)="runDeviceValidation($event)"
      (shareLinkRevoke)="revokeShareLink($event)"
      (releaseReadinessRunRequested)="runReleaseReadiness()"
      (knowledgeSettingsSave)="saveKnowledgeSettings($event)"
      (knowledgeIndexRunRequested)="runKnowledgeIndex()"
      (knowledgeIndexJobCancelRequested)="cancelKnowledgeIndexJob($event)"
      (localModelDownloadRequested)="startLocalModelDownload($event)"
      (localModelDownloadCancelRequested)="cancelLocalModelDownload($event)"
      (filesBrowseRequested)="browseFiles($event)"
    ></hd-page-state-panel>
  `
})
export class DeskPageComponent {
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(HarborDeskAdminApiService);
  private readonly refresh$ = new BehaviorSubject(0);

  protected savingMemberId: string | null = null;
  protected saveError: string | null = null;
  protected saveSuccess: string | null = null;
  protected testingEndpointId: string | null = null;
  protected endpointTestResults: Record<string, ModelEndpointTestResult> = {};
  protected savingTargetId: string | null = null;
  protected deletingTargetId: string | null = null;
  protected deviceActionBusyKey: string | null = null;
  protected deviceActionResults: Record<string, string> = {};
  protected rtspCheckResults: Record<string, RtspCheckResult> = {};
  protected releaseReadinessBusy = false;
  protected knowledgeIndexBusy = false;
  protected knowledgeIndexJobBusyId: string | null = null;
  protected modelDownloadBusyId: string | null = null;
  protected filesBrowse: FilesBrowseResponse | null = null;

  protected readonly state$ = combineLatest([this.route.data, this.refresh$]).pipe(
    switchMap(([data]) => this.api.observePage(data['pageId'] as HarborDeskPageId))
  );

  protected updateDefaultDeliverySurface(userId: string, surface: DeliverySurface): void {
    this.savingMemberId = userId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .updateDefaultDeliverySurface(userId, surface)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text(`Default proactive surface saved as ${surface}.`, `默认主动通知通道已保存为 ${surface}。`);
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.savingMemberId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            this.text('Failed to save the default proactive surface.', '保存默认主动通知通道失败。');
          this.saveSuccess = null;
        }
      });
  }

  protected runEndpointTest(modelEndpointId: string): void {
    this.testingEndpointId = modelEndpointId;
    this.api
      .testModelEndpoint(modelEndpointId)
      .pipe(
        tap((result) => {
          this.endpointTestResults = {
            ...this.endpointTestResults,
            [modelEndpointId]: result
          };
        }),
        finalize(() => {
          this.testingEndpointId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.endpointTestResults = {
            ...this.endpointTestResults,
            [modelEndpointId]: {
              ok: false,
              status: 'degraded',
              summary:
                (error?.error?.error?.message as string | undefined) ??
                (error?.error?.message as string | undefined) ??
                error?.message ??
                this.text('Endpoint test failed.', '模型端点测试失败。'),
              endpoint: {
                model_endpoint_id: modelEndpointId,
                model_kind: 'unknown',
                endpoint_kind: 'unknown',
                provider_key: 'unknown',
                model_name: 'unknown',
                capability_tags: [],
                cost_policy: {},
                status: 'degraded',
                metadata: {}
              }
            }
          };
        }
      });
  }

  protected setDefaultNotificationTarget(targetId: string): void {
    this.savingTargetId = targetId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .setDefaultNotificationTarget(targetId)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text('Default notification target updated.', '默认通知目标已更新。');
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.savingTargetId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            this.text('Failed to update the default notification target.', '更新默认通知目标失败。');
          this.saveSuccess = null;
        }
      });
  }

  protected deleteNotificationTarget(targetId: string): void {
    this.deletingTargetId = targetId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .deleteNotificationTarget(targetId)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text('Notification target deleted.', '通知目标已删除。');
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.deletingTargetId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            this.text('Failed to delete the notification target.', '删除通知目标失败。');
          this.saveSuccess = null;
        }
      });
  }

  protected scanDevices(payload: DiscoveryScanPayload): void {
    this.runDeviceAction('scan', this.text('Discovery scan finished.', '发现扫描已完成。'), () => this.api.scanDevices(payload));
  }

  protected addManualDevice(payload: ManualDevicePayload): void {
    this.runDeviceAction('manual-add', this.text('Manual device add finished.', '手动添加设备已完成。'), () => this.api.addManualDevice(payload));
  }

  protected setDefaultCamera(deviceId: string): void {
    this.runDeviceAction(`${deviceId}:default`, this.text('Default camera updated.', '默认摄像头已更新。'), () => this.api.setDefaultCamera(deviceId), deviceId);
  }

  protected saveDeviceCredentials(deviceId: string, payload: DeviceCredentialsPayload): void {
    this.runDeviceAction(
      `${deviceId}:credentials`,
      this.text('Credentials saved as redacted configured status.', '凭据已保存，界面仅显示 redacted/configured 状态。'),
      () => this.api.saveDeviceCredentials(deviceId, payload),
      deviceId
    );
  }

  protected checkDeviceRtsp(deviceId: string, payload: RtspCheckPayload): void {
    this.deviceActionBusyKey = `${deviceId}:rtsp`;
    this.saveError = null;
    this.api
      .checkDeviceRtsp(deviceId, payload)
      .pipe(
        tap((result) => {
          this.rtspCheckResults = {
            ...this.rtspCheckResults,
            [deviceId]: result
          };
          this.deviceActionResults = {
            ...this.deviceActionResults,
            [deviceId]: result.reachable ? this.text('RTSP reachable.', 'RTSP 可达。') : result.error_message || this.text('RTSP check failed.', 'RTSP 检查失败。')
          };
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.deviceActionBusyKey = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError = this.errorMessage(error, this.text('RTSP check failed.', 'RTSP 检查失败。'));
        }
      });
  }

  protected testCameraSnapshot(deviceId: string): void {
    this.runDeviceAction(`${deviceId}:snapshot`, this.text('Snapshot task queued.', '快照任务已提交。'), () => this.api.createCameraSnapshotTask(deviceId), deviceId);
  }

  protected createCameraShareLink(deviceId: string): void {
    this.runDeviceAction(`${deviceId}:share`, this.text('Share link created.', '分享链接已创建。'), () => this.api.createCameraShareLink(deviceId), deviceId);
  }

  protected runDeviceValidation(deviceId: string): void {
    this.deviceActionBusyKey = `${deviceId}:validation`;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .runDeviceValidation(deviceId, { scope: 'all', reason: 'harbordesk-devices-aiot' })
      .pipe(
        tap((result) => {
          const message = result.summary ?? this.text('Device validation run accepted.', '设备验收运行请求已接收。');
          this.saveSuccess = message;
          this.deviceActionResults = {
            ...this.deviceActionResults,
            [deviceId]: message
          };
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.deviceActionBusyKey = null;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Device validation run failed.', '设备验收运行失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected revokeShareLink(shareLinkId: string): void {
    this.runDeviceAction(`${shareLinkId}:revoke`, this.text('Share link revoked.', '分享链接已撤销。'), () => this.api.revokeShareLink(shareLinkId));
  }

  protected runReleaseReadiness(): void {
    this.releaseReadinessBusy = true;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .runReleaseReadiness({ scope: 'all', reason: 'harbordesk-overview' })
      .pipe(
        tap((result) => {
          this.saveSuccess = result.summary || this.text('Release readiness run accepted.', '发布 readiness 运行请求已接收。');
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.releaseReadinessBusy = false;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Release readiness run failed.', '发布 readiness 运行失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected saveKnowledgeSettings(payload: KnowledgeSettings): void {
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .saveKnowledgeSettings(payload)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text('Knowledge settings saved.', '知识库配置已保存。');
          this.refresh$.next(Date.now());
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Failed to save knowledge settings.', '保存知识库配置失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected runKnowledgeIndex(): void {
    this.knowledgeIndexBusy = true;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .runKnowledgeIndex()
      .pipe(
        tap((result) => {
          this.saveSuccess = this.knowledgeIndexRunMessage(result);
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.knowledgeIndexBusy = false;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Knowledge index run failed.', '知识库索引运行失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected cancelKnowledgeIndexJob(jobId: string): void {
    this.knowledgeIndexJobBusyId = jobId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .cancelKnowledgeIndexJob(jobId)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text('Knowledge index job cancel requested.', '知识库索引任务取消请求已提交。');
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.knowledgeIndexJobBusyId = null;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Failed to cancel knowledge index job.', '取消知识库索引任务失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected startLocalModelDownload(model: LocalModelCatalogItem): void {
    const modelId = model.model_id;
    const payload: StartLocalModelDownloadRequest = {
      model_id: modelId,
      display_name: model.display_name ?? model.label ?? modelId,
      provider_key: model.provider_key ?? model.provider ?? 'local',
      target_path: null,
      metadata: {
        source_kind: model.source_kind ?? 'huggingface',
        repo_id: model.repo_id ?? model.model_id,
        revision: model.revision ?? 'main',
        file_policy: model.file_policy ?? 'runtime_snapshot'
      }
    };
    this.modelDownloadBusyId = modelId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .startLocalModelDownload(payload)
      .pipe(
        tap((result) => {
          const jobId = result.job?.job_id ?? result.job_id;
          this.saveSuccess = this.text(
            `Model download queued: ${jobId}.`,
            `模型下载任务已提交：${jobId}。`
          );
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.modelDownloadBusyId = null;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Model download request failed.', '模型下载请求失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected cancelLocalModelDownload(jobId: string): void {
    this.modelDownloadBusyId = jobId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .cancelLocalModelDownload(jobId)
      .pipe(
        tap(() => {
          this.saveSuccess = this.text('Model download cancel requested.', '模型下载取消请求已提交。');
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.modelDownloadBusyId = null;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, this.text('Failed to cancel model download.', '取消模型下载失败。'));
          this.saveSuccess = null;
        }
      });
  }

  protected browseFiles(path: string | null): void {
    this.saveError = null;
    this.api.browseFiles(path).subscribe({
      next: (result) => {
        this.filesBrowse = result;
      },
      error: (error: unknown) => {
        this.saveError = this.errorMessage(error, this.text('Failed to browse files.', '浏览文件失败。'));
      }
      });
  }

  private knowledgeIndexRunMessage(result: KnowledgeIndexRunResponse): string {
    const jobIds = result.job_ids ?? [];
    const jobSuffix = jobIds.length ? ` Job(s): ${jobIds.join(', ')}.` : '';
    const followUp = jobIds.length
      ? this.text(' Track progress in Index jobs.', ' 请在索引任务列表跟踪进度。')
      : this.text(' Check RAG readiness and index status.', ' 请检查 RAG readiness 和索引状态。');

    if (result.errors.length) {
      return (
        this.text(
          `Knowledge index refresh request returned ${result.errors.length} error(s).${jobSuffix}`,
          `知识库索引刷新请求返回 ${result.errors.length} 个错误。${jobSuffix}`
        ) + followUp
      );
    }

    if (result.status === 'queued' && jobIds.length) {
      return (
        this.text(
          `Knowledge index refresh queued for ${result.root_count} source root(s).${jobSuffix}`,
          `知识库索引刷新已排队，覆盖 ${result.root_count} 个源目录。${jobSuffix}`
        ) + followUp
      );
    }

    if (!jobIds.length) {
      return this.text(
        'Knowledge index refresh request accepted, but no jobs were returned. Check RAG readiness and index status.',
        '知识库索引刷新请求已接收，但未返回任务。请检查 RAG readiness 和索引状态。'
      );
    }

    return (
      this.text(
        `Knowledge index refresh request accepted with status ${result.status}.${jobSuffix}`,
        `知识库索引刷新请求已接收，状态为 ${result.status}。${jobSuffix}`
      ) + followUp
    );
  }

  private runDeviceAction<T>(
    busyKey: string,
    successMessage: string,
    requestFactory: () => Observable<T>,
    deviceId?: string
  ): void {
    this.deviceActionBusyKey = busyKey;
    this.saveError = null;
    this.saveSuccess = null;
    requestFactory()
      .pipe(
        tap(() => {
          this.saveSuccess = successMessage;
          if (deviceId) {
            this.deviceActionResults = {
              ...this.deviceActionResults,
              [deviceId]: successMessage
            };
          }
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.deviceActionBusyKey = null;
        })
      )
      .subscribe({
        error: (error: unknown) => {
          this.saveError = this.errorMessage(error, successMessage);
          this.saveSuccess = null;
        }
      });
  }

  private errorMessage(error: unknown, fallback: string): string {
    const maybe = error as { error?: { error?: { message?: string }; message?: string } | string; message?: string };
    if (typeof maybe?.error === 'string' && maybe.error.trim()) {
      return maybe.error;
    }
    const payload = typeof maybe?.error === 'object' && maybe.error !== null ? maybe.error : undefined;
    return (
      payload?.error?.message ??
      payload?.message ??
      maybe?.message ??
      fallback
    );
  }

  private text(english: string, chinese: string): string {
    return uiText(english, chinese);
  }
}
