import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { MatCardModule } from '@angular/material/card';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';

import {
  Channel,
  CHANNEL_META,
  ChannelConfig,
  ConnectivityResult,
} from '../../../interfaces/harborbeacon-settings.interface';

@Component({
  selector: 'ix-harborbeacon-connectivity-test',
  standalone: true,
  imports: [
    CommonModule,
    MatCardModule,
    MatIconModule,
    MatButtonModule,
    MatProgressSpinnerModule,
  ],
  templateUrl: './connectivity-test.component.html',
  styleUrls: ['./connectivity-test.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ConnectivityTestComponent {
  @Input() channels: ChannelConfig[] = [];
  @Input() results: ConnectivityResult[] = [];
  @Input() loading = false;
  @Output() runTest = new EventEmitter<Channel | null>(); // null = test all

  readonly meta = CHANNEL_META;

  get enabledChannels(): ChannelConfig[] {
    return this.channels.filter((c) => c.enabled);
  }

  resultFor(channel: Channel): ConnectivityResult | undefined {
    return this.results.find((r) => r.channel === channel);
  }

  channelLabel(channel: Channel): string {
    return this.meta.find((m) => m.channel === channel)?.label ?? channel;
  }

  channelIcon(channel: Channel): string {
    return this.meta.find((m) => m.channel === channel)?.icon ?? 'settings';
  }

  onTestAll(): void {
    this.runTest.emit(null);
  }

  onTestOne(channel: Channel): void {
    this.runTest.emit(channel);
  }
}
