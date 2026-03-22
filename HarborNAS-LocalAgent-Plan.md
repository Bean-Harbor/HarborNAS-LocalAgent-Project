# HarborNAS 本地智能体规划文档

## 1. 项目目标

为 HarborNAS 构建一个 **混合计算智能体**，具备：
- ✅ **多模态 RAG** - 支持文本、图像、音频、视频的检索增强生成
- ✅ **智能任务编排** - 动态判断任务复杂度，选择最优执行路径
- ✅ **本地优先策略** - 隐私优先，敏感任务不出本地
- ✅ **云边协作** - 复杂任务脱敏后调用云模型
- ✅ **可观测性** - 任务流转过程完全可追溯

---

## 2. 核心架构设计

### 2.1 三层编排框架

```
┌─────────────────────────────────────────────────────────────┐
│           IM 接入层 (HarborClaw — ZeroClaw 二次开发)         │
│  飞书 | 企微 | Telegram | Discord | 钉钉 | Slack | MQTT    │
│  channels.py → 意图解析 → mcp_adapter / autonomy            │
└───────────────────┬─────────────────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────────────────┐
│              用户交互层 (Task Intake)                        │
│  - WebUI 对话入口  - API Gateway  - 消息队列              │
└───────────────────┬─────────────────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────────────────┐
│           智能决策层 (Task Router)                          │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  • 复杂度评估器 (Complexity Assessor)                │  │
│  │  • 隐私风险分类器 (Privacy Classifier)               │  │
│  │  • 资源需求预测器 (Resource Predictor)              │  │
│  │  • 路由决策引擎 (Routing Engine)                     │  │
│  └──────────────────────────────────────────────────────┘  │
└───────┬─────────────────┬──────────────────┬────────────────┘
        │                 │                  │
        ▼                 ▼                  ▼
   ┌─────────────┐  ┌──────────────┐  ┌─────────────────┐
   │ 本地执行器   │  │ 混合执行器    │  │ 云协作执行器     │
   │(L1-Simple)  │  │(L2-Medium)   │  │(L3-Complex)     │
   └─────────────┘  └──────────────┘  └─────────────────┘
        │                 │                  │
        └────────┬────────┴──────────┬───────┘
                 │                  │
         ┌───────▼──────────┐   ┌───▼────────────┐
         │  本地 RAG + 推理  │   │ 脱敏 + 云推理  │
         │  • Ollama/LLaMA  │   │ • 数据脱敏器   │
         │  • LocalAI       │   │ • 云API调用    │
         │  • Vector DB     │   │ • 结果转换     │
         └──────────────────┘   └────────────────┘
```

### 2.2 任务分类与路由规则

| 任务级别 | 场景示例 | 本地处理 | 云端处理 | 执行器 |
|---------|---------|---------|---------|--------|
| **L1-Simple** | 简单文本理解、本地文件查询、基础搜索 | ✅ | ❌ | LocalLLM |
| **L2-Medium** | 多文件分析、本地+云混合推理、审核任务 | ✅ 预处理+编排 | ✅ 部分 | HybridExecutor |
| **L3-Complex** | 深度分析、多轮推理、实时翻译、视频字幕 | ❌ 脱敏+上报 | ✅ | CloudExecutor |

### 2.3 多模态 RAG 体系

```
┌──────────────────────────────────────────────────────────┐
│                 多模态数据接入层                        │
│  ┌─────────┬─────────┬─────────┬──────────┐              │
│  │  文本   │   图像  │   音频  │   视频   │              │
│  └─────────┴─────────┴─────────┴──────────┘              │
└──────────────┬───────────────────────────────────────────┘
               │
        ┌──────▼──────────┐
        │ 多模态向量化    │
        │ • CLIP (图文)   │
        │ • Whisper (音)  │
        │ • Text Embed    │
        └──────┬──────────┘
               │
        ┌──────▼──────────────────────────────┐
        │ 混合向量数据库                      │
        │ • Milvus / Weaviate                │
        │ • 支持向量+标量混合查询             │
        │ • 本地 SQLite (轻量)                │
        └──────┬───────────────────────────────┘
               │
        ┌──────▼──────────────────────────────┐
        │ 语义检索 + 重排序                    │
        │ • 密集向量检索 (DPR)                │
        │ • BM25 稀疏检索 (混合)              │
        │ • LLM-Based 重排 (可选云端)         │
        └───────────────────────────────────┘
```

---

## 3. 核心模块详细设计

### 3.1 任务复杂度评估器 (Complexity Assessor)

**输入**: 用户查询 + 上下文
**输出**: 复杂度评分 (0-100) + 建议操作

```python
# 伪代码
class ComplexityAssessor:
    def evaluate(self, task):
        score = 0
        
        # 因素1: 查询长度 (0-10)
        score += min(len(task) / 1000, 10)
        
        # 因素2: 需要的数据模态数 (0-20)
        # 文字(5) + 图像(5) + 音频(5) + 视频(5)
        score += len(task.required_modalities) * 5
        
        # 因素3: 推理步数 (0-20)
        score += task.required_reasoning_steps * 2
        
        # 因素4: 需要实时信息 (0-20)
        if task.needs_realtime:
            score += 20
        
        # 因素5: 多语言处理 (0-15)
        if task.num_languages > 1:
            score += task.num_languages * 5
        
        # 因素6: 定制模型需求 (0-15)
        if task.requires_finetuned_model:
            score += 15
        
        return min(score, 100)
    
    def get_routing_decision(self, score):
        if score < 30:
            return "LOCAL"
        elif score < 70:
            return "HYBRID"
        else:
            return "CLOUD"
```

### 3.2 隐私风险分类器 (Privacy Classifier)

**核心逻辑**: PII 检测 + 数据敏感度评分

```python
class PrivacyClassifier:
    def classify(self, task_data):
        pii_entities = self.detect_pii(task_data)  # PII检测
        sensitivity_score = self.rate_sensitivity(task_data)  # 敏感度评分
        
        if pii_entities and sensitivity_score > 0.7:
            return {
                'confidence': 'HIGH',
                'action': 'ANONYMIZE_THEN_CLOUD',
                'pii_found': pii_entities
            }
        elif sensitivity_score > 0.5:
            return {
                'confidence': 'MEDIUM',
                'action': 'ASK_USER',
                'reason': 'Sensitive data detected'
            }
        else:
            return {
                'confidence': 'LOW',
                'action': 'CAN_PROCESS_LOCALLY'
            }
```

**PII 检测清单**:
- 身份证号、护照号
- 电话号码、邮箱
- 位置信息、家庭地址
- 财务数据 (银行卡、社会保险号)
- 医疗信息
- 面部特征 (如有图像)

### 3.3 智能路由决策引擎 (Routing Engine)

```
决策树:
├─ PII 检测
│  ├─ YES → 脱敏处理
│  │       ├─ 本地可处理 → L1_LOCAL
│  │       └─ 需云端 → ANONYMIZE_THEN_CLOUD
│  └─ NO
│      ├─ 复杂度评分
│      │  ├─ < 30 → L1_LOCAL
│      │  ├─ 30-70 → L2_HYBRID
│      │  └─ > 70 → L3_CLOUD
│      └─ 本地资源
│         ├─ 充足 → 降级到本地执行
│         └─ 不足 → 升级到云端或混合
```

### 3.4 数据脱敏器 (Data Anonymizer)

**脱敏策略**:
1. **结构化脱敏**: PII 字段替换为占位符
2. **语义脱敏**: 保留语义，替换具体值
3. **差分隐私**: 添加噪声保护个体隐私

```python
class DataAnonymizer:
    def anonymize(self, data, sensitivity_level='HIGH'):
        # 第一步: 检测PII
        pii_map = self.detect_and_map_pii(data)
        
        # 第二步: 脱敏
        anonymized_data = data
        for pii_type, entities in pii_map.items():
            for entity in entities:
                placeholder = self.get_placeholder(pii_type)
                anonymized_data = anonymized_data.replace(
                    entity, placeholder
                )
        
        # 第三步: 记录映射 (本地密钥保管)
        self.store_mapping_securely(pii_map)
        
        # 第四步: 返回脱敏数据+反向映射密钥
        return {
            'data': anonymized_data,
            'mapping_key': self.encrypt_mapping(pii_map)
        }
    
    def deanonymize_response(self, cloud_response, mapping_key):
        """云端返回结果后，恢复原始信息"""
        pii_map = self.decrypt_mapping(mapping_key)
        result = cloud_response
        for pii_type, mappings in pii_map.items():
            for placeholder, original in mappings.items():
                result = result.replace(placeholder, original)
        return result
```

### 3.5 本地执行器 (Local Executor)

**组件**:
- **推理引擎**: Ollama/LLaMA 2/Mistral
- **向量数据库**: Milvus (中等规模) 或 FAISS (轻量)
- **可视化模型**: CLIP for 图像理解
- **音频处理**: Whisper (本地推理) 或 offlineASR

```yaml
LocalExecutor:
  models:
    text_generation:
      - mistral:7b
      - llama2:13b
      - zephyr:7b
    embeddings:
      - sentence-transformers/all-MiniLM-L6-v2
    multimodal:
      - openai/clip-vit-base-patch32
    speech:
      - openai/whisper-base
  
  vector_db:
    type: milvus  # 本地部署
    config:
      dimension: 384
      metric_type: L2
      
  max_tokens: 2048
  timeout: 30s
  gpu_allocation: 70%  # 留出余量给系统
```

### 3.6 混合执行器 (Hybrid Executor)

**场景**: 预处理+本地初步分析 → 云端精细分析 → 本地后处理

```python
class HybridExecutor:
    async def execute(self, task):
        # 步骤1: 本地预处理
        preprocessed = await self.local_executor.preprocess(task)
        
        # 步骤2: 评估是否需要云端
        if self.should_call_cloud(preprocessed):
            # 步骤3: 脱敏
            anonymized = self.anonymizer.anonymize(preprocessed)
            
            # 步骤4: 云端推理
            cloud_result = await self.cloud_executor.infer(
                anonymized['data'],
                context=preprocessed['context']
            )
            
            # 步骤5: 反脱敏
            result = self.anonymizer.deanonymize_response(
                cloud_result,
                anonymized['mapping_key']
            )
        else:
            result = preprocessed
        
        # 步骤6: 本地后处理
        final_result = await self.local_executor.postprocess(result)
        
        return final_result
```

### 3.7 云端执行器 (Cloud Executor)

**API 抽象层**, 支持多个云服务:
- OpenAI API (GPT-4/3.5)
- Claude (Anthropic)
- 本地私有云部署

```python
class CloudExecutor:
    def __init__(self, config):
        self.providers = {
            'openai': OpenAIProvider(config.openai_key),
            'anthropic': AnthropicProvider(config.anthropic_key),
            'private_cloud': PrivateCloudProvider(config.endpoint)
        }
        self.request_log = RequestLogger(config.audit_db)
    
    async def infer(self, anonymized_data, context=None):
        # 选择提供商 (可负载均衡)
        provider = self.select_provider()
        
        try:
            response = await provider.call(
                data=anonymized_data,
                context=context,
                model_name=self.config.model
            )
            
            # 审计日志: 记录脱敏状态, 时间戳, 使用量
            self.request_log.log({
                'timestamp': now(),
                'task_id': context['task_id'],
                'anonymized': True,
                'provider': provider.name,
                'tokens_used': response.usage.total_tokens,
                'latency_ms': response.latency
            })
            
            return response.text
        except Exception as e:
            self.handle_cloud_failure(e, context)
            raise
```

---

## 4. 多模态 RAG 实现

### 4.1 数据摄入管道

```
HarborNAS 文件系统
  └─ 文本: Markdown, PDF, TXT
     └─ OCR 提取 (local: PaddleOCR)
     └─ 分块策略: 递归分块 (segment_size=512, overlap=50)
     └─ 向量化: sentence-transformers
     └─ 写入向量DB
     
  └─ 图像: JPG, PNG, WEBP
     └─ 视觉理解 (CLIP)
     └─ 对象检测 (YOLO)
     └─ 元数据提取 (EXIF, 标签)
     └─ 多向量存储 (CLIP emb + metadata)
     
  └─ 音频: MP3, WAV, M4A
     └─ 转录 (Whisper)
     └─ 情感分析 (sentiment-transformers)
     └─ 向量化 (音频特征)
     └─ 存储文本副本 + 音频向量
     
  └─ 视频: MP4, MKV, AVI
     └─ 关键帧提取
     └─ 场景分割
     └─ 字幕/字幕生成 (Whisper on audio track)
     └─ 图像特征 (per frame)
     └─ 音频转录
     └─ 存储: 时间线索引 + 多模态向量
```

### 4.2 查询理解与检索

```
用户查询: "给我看最近拍的包含人物的照片，对吧们讲一下故事"

       ↓
   查询解析器
   ├─ 实体抽取: ["照片", "人物", "最近"]
   ├─ 意图识别: SEARCH + SUMMARIZE
   └─ 模态需求: IMAGE + TEXT
   
       ↓
   多模态检索
   ├─ 图像检索 (CLIP: "photo with people")
   │  └─ 返回 Top-K 图像 + metadata
   ├─ 时间过滤: "最近" → 时间范围
   ├─ 对象过滤: objects.contains("person")
   │  └─ 使用 YOLO 检测结果
   └─ 重排序
      └─ LLM 相关性评分 (可选)
   
       ↓
   上下文增强
   ├─ 提取关联的文本 (eg. 日记、标签)
   ├─ 构建 RAG 上下文
   └─ 添加时间线索引
   
       ↓
   生成响应
   ├─ 本地LLM 生成故事
   ├─ 引用原始文件
   └─ 返回结果 + 多模态预览
```

### 4.3 向量数据库 Schema

```sql
-- 向量表
CREATE TABLE vectors (
    id BIGINT PRIMARY KEY,
    namespace VARCHAR(50),  -- "text", "image", "audio", "video"
    embedding FLOAT32[384],  -- 向量
    source_file_id BIGINT,
    chunk_id INT,
    metadata JSON,  -- 时间戳、标签、尺寸等
    created_at TIMESTAMP,
    ttl INTERVAL  -- 自动过期
);

-- 文件索引
CREATE TABLE files (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255),
    path VARCHAR(1024),
    type ENUM('text', 'image', 'audio', 'video', 'document'),
    size_bytes BIGINT,
    hash_sha256 VARCHAR(64),  -- 去重
    created_at TIMESTAMP,
    modified_at TIMESTAMP,
    indexed_at TIMESTAMP
);

-- 任务日志 (可观测性)
CREATE TABLE task_logs (
    id UUID PRIMARY KEY,
    task_type VARCHAR(50),
    status ENUM('pending', 'processing', 'completed', 'failed'),
    complexity_score INT,
    routing_decision VARCHAR(20),  -- LOCAL/HYBRID/CLOUD
    input_hash VARCHAR(64),
    output_summary VARCHAR(255),
    local_latency_ms INT,
    cloud_latency_ms INT,
    tokens_used INT,
    cost DECIMAL(10, 6),
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    error_message TEXT
);
```

---

## 5. 技术栈推荐

### 5.1 核心依赖

| 组件 | 推荐方案 | 备选方案 |
|-----|---------|---------|
| 本地推理 | Ollama | LocalAI, ONNX Runtime |
| 向量DB | Milvus | Weaviate, FAISS, Pinecone |
| 嵌入模型 | sentence-transformers | BGE, ONNX-optimized |
| 多模态 | CLIP, Whisper | MediaPipe, TorchVision |
| Web框架 | FastAPI | Flask, Django |
| 日志/追踪 | OpenTelemetry | ELK stack |
| 消息队列 | Redis Queue | Celery + RabbitMQ |
| 向量搜索 | Milvus | Elasticsearch |

### 5.2 目录结构建议

```
harbor-local-agent/
├── README.md
├── architecture.md
├── requirements.txt
│
├── src/
│   ├── __init__.py
│   ├── main.py                    # 入口点
│   │
│   ├── core/
│   │   ├── __init__.py
│   │   ├── router.py              # 路由决策引擎
│   │   ├── complexity_assessor.py
│   │   └── privacy_classifier.py
│   │
│   ├── executors/
│   │   ├── __init__.py
│   │   ├── base_executor.py
│   │   ├── local_executor.py
│   │   ├── hybrid_executor.py
│   │   └── cloud_executor.py
│   │
│   ├── rag/
│   │   ├── __init__.py
│   │   ├── multimodal_ingester.py  # 多模态数据摄入
│   │   ├── retriever.py            # 检索
│   │   ├── vector_store.py         # 向量DB 封装
│   │   └── query_parser.py         # 查询理解
│   │
│   ├── security/
│   │   ├── __init__.py
│   │   ├── anonymizer.py           # 数据脱敏
│   │   ├── pii_detector.py         # PII 检测
│   │   └── key_manager.py          # 密钥管理
│   │
│   ├── models/
│   │   ├── __init__.py
│   │   ├── local_models.py         # Ollama 集成
│   │   └── cloud_models.py         # 云 API 集成
│   │
│   ├── monitoring/
│   │   ├── __init__.py
│   │   ├── logger.py
│   │   ├── metrics.py              # Prometheus
│   │   └── tracer.py               # OpenTelemetry
│   │
│   ├── api/
│   │   ├── __init__.py
│   │   ├── router.py
│   │   ├── schemas.py              # Pydantic 模型
│   │   └── handlers.py
│   │
│   └── utils/
│       ├── __init__.py
│       ├── config.py
│       └── helpers.py
│
├── tests/
│   ├── __init__.py
│   ├── unit/
│   ├── integration/
│   └── e2e/
│
├── config/
│   ├── local.yaml                  # 本地开发配置
│   ├── prod.yaml                   # 生产配置
│   └── secrets.example.yaml         # 密钥模板
│
├── docker/
│   ├── Dockerfile
│   ├── docker-compose.yaml
│   └── .env.example
│
├── docs/
│   ├── architecture.md
│   ├── api_reference.md
│   ├── deployment.md
│   └── troubleshooting.md
│
└── scripts/
    ├── setup.sh                    # 初始化脚本
    ├── download_models.sh          # 模型下载
    └── migrate_db.sh               # 数据库迁移
```

---

## 6. 实现阶段规划

### Phase 1: 核心框架 (Weeks 1-2)
- [ ] 设计路由决策引擎
- [ ] 实现本地执行器 (Ollama 集成)
- [ ] 基础日志系统
- [ ] API 框架搭建

### Phase 2: 数据脱敏与隐私 (Weeks 3-4)
- [ ] PII 检测模块
- [ ] 数据脱敏器
- [ ] 密钥管理系统
- [ ] 审计日志

### Phase 3: 多模态 RAG (Weeks 5-7)
- [ ] 向量 DB 集成 (Milvus)
- [ ] 文本摄入管道
- [ ] 图像摄入管道 (CLIP)
- [ ] 音频处理 (Whisper)
- [ ] 视频索引 (关键帧)
- [ ] 混合查询检索

### Phase 4: 云协作与混合执行 (Weeks 8-9)
- [ ] 云 API 适配层
- [ ] 混合执行器实现
- [ ] 结果融合策略
- [ ] 容错与重试机制

### Phase 5: 性能优化与部署 (Weeks 10-12)
- [ ] 模型量化 (4-bit, 8-bit)
- [ ] 批处理与异步任务
- [ ] 缓存策略
- [ ] Docker 部署
- [ ] 性能基准测试

### Phase 6: 测试与文档 (Weeks 13-14)
- [ ] 单元测试覆盖
- [ ] 集成测试
- [ ] E2E 测试
- [ ] API 文档
- [ ] 部署指南

---

## 7. 关键指标与 SLA

### 7.1 性能目标

| 指标 | 目标 | 说明 |
|-----|------|------|
| 本地查询延迟 | < 500ms | P95 |
| 混合执行延迟 | < 3s | P95 |
| 向量检索准确率 | > 85% | NDCG@10 |
| 数据脱敏精度 | > 99% | 无遗漏 PII |
| 系统可用性 | > 99% | excludes 云依赖 |
| 隐私合规率 | 100% | 审计通过 |

### 7.2 可观测性指标

```python
metrics = {
    'routing': {
        'local_ratio': 'Gauge',  # 本地执行占比
        'cloud_ratio': 'Gauge',  # 云执行占比
        'hybrid_ratio': 'Gauge'
    },
    'performance': {
        'local_latency': 'Histogram',
        'cloud_latency': 'Histogram',
        'e2e_latency': 'Histogram'
    },
    'privacy': {
        'pii_detected_count': 'Counter',
        'anonymizations_performed': 'Counter',
        'failed_anonymizations': 'Counter'
    },
    'rag': {
        'retrieval_recall': 'Gauge',
        'retrieval_precision': 'Gauge',
        'avg_documents_retrieved': 'Gauge'
    }
}
```

---

## 8. 风险与缓解策略

| 风险 | 影响 | 缓解策略 |
|------|------|---------|
| 本地模型不够强大 | L3 任务质量低 | 定期微调 + 云补充 |
| 脱敏不完全 | 隐私泄露 | 多层检测 + 人工审核 |
| 网络不稳定 | 云组件失败 | 本地降级 + 队列缓冲 |
| 向量检索性能 | 大规模数据慢 | 分区 + 层级索引 |
| 成本失控 | 云调用过多 | 配额限制 + 智能路由 |

---

## 9. 下一步行动

### 立即行动 (Week 0)
1. **技术选型确认** - 确定具体模型、DB、云服务商
2. **环境搭建** - Docker Compose 本地开发环境
3. **代码库初始化** - Git 仓库 + CI/CD 流程
4. **团队熟悉** - 阅读 HarborNAS 源码架构

### Week 1 优先事项
- 完成 `core/router.py` 原型 (路由决策引擎)
- Ollama 本地模型部署测试
- 第一个 API 端点 (Task Intake)

### 风险检查清单
- [ ] 本地推理延迟是否可接受？ (需要 benchmark)
- [ ] 云 API 成本是否在预算内？ (需要定价方案)
- [ ] 脱敏流程是否符合法规？ (GDPR/CCPA/中国隐私法)
- [ ] 向量 DB 可扩展性？ (模拟百万级数据)

