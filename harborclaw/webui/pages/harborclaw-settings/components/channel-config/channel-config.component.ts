import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatCardModule } from '@angular/material/card';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatExpansionModule } from '@angular/material/expansion';

import {
  Channel,
  ChannelConfig,
  ChannelMeta,
  CHANNEL_META,
} from '../../../interfaces/harborclaw-settings.interface';

@Component({
  selector: 'ix-harborclaw-channel-config',
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    MatCardModule,
    MatSlideToggleModule,
    MatFormFieldModule,
    MatInputModule,
    MatIconModule,
    MatButtonModule,
    MatExpansionModule,
  ],
  templateUrl: './channel-config.component.html',
  styleUrls: ['./channel-config.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChannelConfigComponent {
  @Input() channels: ChannelConfig[] = [];
  @Output() channelsChange = new EventEmitter<ChannelConfig[]>();
  @Output() testChannel = new EventEmitter<{ channel: Channel; config: ChannelConfig }>();

  readonly meta: ChannelMeta[] = CHANNEL_META;

  getConfig(channel: Channel): ChannelConfig {
    return (
      this.channels.find((c) => c.channel === channel) ?? {
        channel,
        enabled: false,
        extra: {},
      }
    );
  }

  onToggle(channel: Channel, enabled: boolean): void {
    this.updateChannel(channel, { enabled });
  }

  onFieldChange(channel: Channel, field: string, value: string): void {
    if (field.startsWith('extra.')) {
      const key = field.slice(6);
      const cfg = this.getConfig(channel);
      this.updateChannel(channel, { extra: { ...cfg.extra, [key]: value } });
    } else {
      this.updateChannel(channel, { [field]: value });
    }
  }

  onTest(channel: Channel): void {
    this.testChannel.emit({ channel, config: this.getConfig(channel) });
  }

  /** Map a credential field to a human-readable label for the form. */
  fieldLabel(field: string): string {
    const map: Record<string, string> = {
      app_id: 'App ID',
      app_secret: 'App Secret',
      bot_token: 'Bot Token',
      webhook_url: 'Webhook URL',
      'extra.broker': 'MQTT Broker',
      'extra.port': 'MQTT Port',
      'extra.topic': 'MQTT Topic',
    };
    return map[field] ?? field;
  }

  /** Return 'password' for secrets, 'text' for others. */
  inputType(field: string): string {
    return ['app_secret', 'bot_token'].includes(field) ? 'password' : 'text';
  }

  /** Read the current value of a field from a ChannelConfig. */
  fieldValue(config: ChannelConfig, field: string): string {
    if (field.startsWith('extra.')) {
      return String(config.extra?.[field.slice(6)] ?? '');
    }
    return String((config as Record<string, unknown>)[field] ?? '');
  }

  // -- internal --

  private updateChannel(channel: Channel, patch: Partial<ChannelConfig>): void {
    const idx = this.channels.findIndex((c) => c.channel === channel);
    const existing: ChannelConfig = idx >= 0
      ? { ...this.channels[idx] }
      : { channel, enabled: false, extra: {} };

    const updated = { ...existing, ...patch };
    const list = [...this.channels];
    if (idx >= 0) {
      list[idx] = updated;
    } else {
      list.push(updated);
    }
    this.channelsChange.emit(list);
  }
}
