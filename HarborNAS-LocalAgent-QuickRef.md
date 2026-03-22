# HarborNAS 本地智能体 - 快速参考指南

## 项目核心概念速览

### 系统三层架构

```
┌─────────────────────────────────┐
│  用户交互层                      │  → WebUI、API、消息队列
├─────────────────────────────────┤
│  智能决策层                      │  → 路由、评估、分类
├─────────────────────────────────┤
│  智能执行层                      │  → 本地/混合/云执行
├─────────────────────────────────┤
│  能力层                          │  → RAG、推理、脱敏
├─────────────────────────────────┤
│  基础设施                        │  → Ollama、Milvus、Redis
└─────────────────────────────────┘
```

### 任务路由决策树

```
任务输入
  ├─ PII 检测 → 有 PII?
  │   ├─ YES → 脱敏后决策
  │   │   ├─ 本地可处理 → LOCAL
  │   │   └─ 需要云 → ANONYMIZE + CLOUD
  │   └─ NO
  │       ├─ 复杂度评分 (0-100)
  │       │   ├─ < 30 → LOCAL
  │       │   ├─ 30-70 → HYBRID
  │       │   └─ > 70 → CLOUD
  └─ 本地资源检查
      ├─ 充足 → 降级到本地
      └─ 不足 → 升级到云端或混合
```

### 执行路由详解

| 路由 | 用途 | 延迟 | 隐私 | 成本 | 适用场景 |
|-----|------|------|------|------|---------|
| **LOCAL** | 本地推理 | 100-500ms | ✅ 最高 | ✅ 0 | 简单Q&A、本地文件搜索 |
| **HYBRID** | 本地+云混合 | 1-3s | ✅ 中 | 💰 中 | 多文件分析、审核任务 |
| **CLOUD** | 云端推理 | 2-10s | ⚠️ 低 | 💰 高 | 深度分析、翻译、字幕生成 |

---

## 多模态 RAG 数据流

### 输入端 (Ingestion)

```
HarborNAS 文件系统
│
├─ 文本 (TXT/PDF/MD)
│  └─ [递归分块] → [向量化] → Milvus
│
├─ 图像 (JPG/PNG/WEBP)
│  └─ [CLIP 嵌入] → [对象检测] → Milvus
│
├─ 音频 (MP3/WAV/M4A)
│  └─ [Whisper 转录] → [分块+向量化] → Milvus
│
└─ 视频 (MP4/MKV/AVI)
   └─ [关键帧提取] → [字幕生成] → [多向量索引] → Milvus
```

### 查询端 (Retrieval)

```
用户查询
  └─ [查询理解] → [多模态检索]
     ├─ 向量检索 (CLIP/Text)
     ├─ 稀疏检索 (BM25)
     ├─ 元数据过滤
     └─ [LLM 重排] (可选云端)
     
     → [上下文构建]
     → [LLM 生成响应]
     → 返回结果
```

---

## 核心模块关键代码片段

### 1️⃣ 路由决策 (Router)

```python
decision = await router.route({
    'query': '...',
    'metadata': {...}
})

# 返回: RoutingDecision
# - route: "LOCAL" | "HYBRID" | "CLOUD"
# - confidence: float (0-1)
# - estimated_latency_ms: int
# - reasoning: str
```

### 2️⃣ PII 检测与脱敏

```python
# 检测 PII
pii_result = pii_detector.detect(text)
# 返回: {'email': [...], 'ssn': [...], ...}

# 脱敏
anonymized, mapping_key = anonymizer.anonymize(text)
# 返回: (脱敏后文本, 加密映射密钥)

# 恢复
original = anonymizer.deanonymize(cloud_result, mapping_key)
```

### 3️⃣ 向量搜索

```python
# 存储向量
await vector_store.insert(
    embeddings=[...],
    texts=['chunk1', 'chunk2'],
    metadata=[{'source': 'file.pdf'}, ...]
)

# 搜索
results = await vector_store.search(
    query_embedding=[...],
    top_k=10,
    namespace='image'  # 可按类型过滤
)
# 返回: [{'text': ..., 'score': ..., 'metadata': ...}, ...]
```

---

## 快速启动指南

### 0️⃣ 环境准备 (5 分钟)

```bash
# 克隆项目
git clone <repo-url>
cd harbor-local-agent

# 创建虚拟环境
python -m venv venv
source venv/bin/activate

# 安装依赖
pip install -r requirements.txt

# 启动 Docker 容器
docker-compose -f docker/docker-compose.dev.yaml up -d

# 等待服务就绪 (~30s)
docker-compose -f docker/docker-compose.dev.yaml ps
```

### 1️⃣ 下载模型 (10-20 分钟)

```bash
# Ollama 模型
ollama pull mistral:7b
ollama pull llama2:13b

# 嵌入模型 (自动下载)
python -c "from sentence_transformers import SentenceTransformer; \
           SentenceTransformer('all-MiniLM-L6-v2')"

# CLIP 模型 (自动下载)
python -c "from transformers import CLIPProcessor, CLIPModel; \
           CLIPModel.from_pretrained('openai/clip-vit-base-patch32')"
```

### 2️⃣ 初始化数据库 (2 分钟)

```bash
# 运行数据库迁移
python scripts/migrate_db.sh

# 建立向量索引
python -m src.rag.vector_store init
```

### 3️⃣ 启动服务 (1 分钟)

```bash
# 开发模式
uvicorn src.api.main:app --reload --host 0.0.0.0 --port 8000

# 访问 API 文档
# http://localhost:8000/docs
```

### 4️⃣ 测试第一个请求 (1 分钟)

```bash
curl -X POST http://localhost:8000/api/v1/tasks \
  -H "Content-Type: application/json" \
  -d '{
    "query": "what is 2+2?",
    "prefer_local": true
  }'

# 响应:
# {
#   "task_id": "uuid",
#   "status": "queued"
# }
```

---

## 关键概念深度解析

### 复杂度评分算法

```
score = 0

# 因素1: 查询长度 (0-10)
score += min(len(query) / 1000, 10)

# 因素2: 所需数据模态数 (0-30)
for modality in ['text', 'image', 'audio', 'video']:
    score += 7.5 if modality_needed else 0

# 因素3: 推理步数 (0-20)
score += min(required_steps * 2, 20)

# 因素4: 实时信息需求 (0-20)
score += 20 if needs_realtime else 0

# 因素5: 多语言处理 (0-15)
score += (num_languages - 1) * 5

# 因素6: 定制化需求 (0-15)
score += 15 if requires_finetuned_model else 0

# 最终分数: 0-100
final_score = min(score, 100)

# 路由决策
if final_score < 30:
    return "LOCAL"
elif final_score < 70:
    return "HYBRID"
else:
    return "CLOUD"
```

### 数据脱敏流程

```
原始数据
  └─ [PII 检测] (正则+上下文)
     └─ PII 清单: {email, phone, ssn, ...}
     
     └─ [脱敏映射]
        SSN "123-45-6789" → "[SSN_a1b2c3d4]"
        Email "john@example.com" → "[EMAIL_e5f6g7h8]"
        
     └─ [加密映射表]
        使用 Fernet 对称加密存储映射关系
        只在本地维护密钥
        
     └─ 脱敏数据 + 加密密钥 → 发送到云端
     
     └─ [云端处理]
        收到的是脱敏内容, 无法反解
        
     └─ [反脱敏]
        接收云端结果
        使用本地密钥解密映射表
        恢复原始值
        返回给用户
```

### 混合执行流程

```
task = {
    'query': '分析这 10 个 PDF 文件中关于 AI 的见解',
    'files': ['file1.pdf', ..., 'file10.pdf'],
    'require_comparison': True
}

步骤 1: 本地预处理
├─ 评估复杂度 → 75 (中-高)
├─ 检测 PII → 未检测到
└─ 预测路由 → HYBRID

步骤 2: 本地初步分析
├─ 加载 Milvus 向量
├─ 检索 AI 相关 chunks (50+ 个)
├─ 提取关键观点 (本地 LLM)
└─ 预处理结果: {summaries: [...], chunks: [...]}

步骤 3: 判断是否需要云端
├─ 检查: 是否需要深度对比?
├─ 检查: 本地能力是否足够?
└─ 决策: 需要云端精细分析

步骤 4: 脱敏并上报
├─ 检测任何残留 PII
├─ 发送脱敏后的预处理结果
└─ 云端: "比较这些观点并生成综合报告"

步骤 5: 云端增强分析
├─ GPT-4 深度分析
├─ 生成交叉对比
└─ 返回精细化报告

步骤 6: 本地后处理
├─ 反脱敏 (无需, 输入已脱敏)
├─ 格式化输出
├─ 添加本地引用
└─ 返回给用户
```

---

## 监控与可观测性

### 关键指标

```yaml
路由指标:
  local_ratio: "本地执行占比(%)"
  hybrid_ratio: "混合执行占比(%)"
  cloud_ratio: "云执行占比(%)"

性能指标:
  local_latency_p95: "本地推理 P95 延迟"
  hybrid_latency_p95: "混合执行 P95 延迟"
  cloud_latency_p95: "云推理 P95 延迟"
  e2e_latency_p95: "端到端 P95 延迟"

隐私指标:
  pii_detection_rate: "PII 检测率(%)"
  anonymization_success_rate: "脱敏成功率(%)"
  deanonymization_success_rate: "反脱敏成功率(%)"

RAG 指标:
  retrieval_recall: "检索召回率"
  retrieval_precision: "检索精度"
  avg_documents_retrieved: "平均检索文档数"
```

### 日志查询

```bash
# 查看最近的任务
SELECT * FROM task_logs 
ORDER BY started_at DESC 
LIMIT 10;

# 查看失败任务
SELECT task_id, error_message, status 
FROM task_logs 
WHERE status = 'failed' 
ORDER BY started_at DESC;

# 路由分布
SELECT routing_decision, count(*) 
FROM task_logs 
GROUP BY routing_decision;

# 成本分析
SELECT 
    date(timestamp) as day,
    sum(cost) as total_cost,
    count(*) as request_count,
    sum(cost) / count(*) as avg_cost_per_request
FROM task_logs
WHERE status = 'completed'
GROUP BY date(timestamp)
ORDER BY day DESC;
```

---

## 故障排查速查表

| 问题 | 可能原因 | 解决方案 |
|------|---------|---------|
| 本地推理超时 (>10s) | Ollama 没启动 / 模型太大 | `ollama serve` / 降级到 7B 模型 |
| 向量检索结果差 | 向量维度不匹配 | 确保所有向量 dim=384 |
| PII 检测遗漏 | 正则模式不全面 | 添加自定义检测器 |
| VectorDB 连接失败 | Milvus 未启动 | `docker-compose up milvus` |
| 云 API 超时 | 网络问题 / API 限流 | 添加重试机制 / 增加超时 |
| 内存溢出 | 批量处理太大 | 减小 batch_size / 启用流式处理 |

---

## 安全最佳实践

### ✅ DO 做

- ✅ 本地维护所有 PII 映射密钥
- ✅ 所有云 API 调用都必须经过脱敏
- ✅ 记录审计日志中的所有敏感操作
- ✅ 定期更新 PII 检测规则
- ✅ 使用强加密算法 (Fernet/AES-256)

### ❌ DON'T 不做

- ❌ 将原始 PII 发送到云端
- ❌ 在日志中记录明文密钥
- ❌ 跳过脱敏甚至一次
- ❌ 硬编码 API 密钥
- ❌ 信任任何外部输入

---

## 下一步行动

### 立即 (今天)

- [ ] 阅读完整的规划文档
- [ ] 克隆 GitHub 仓库 (待创建)
- [ ] 搭建本地开发环境
- [ ] 验证 Docker 容器能启动

### 本周

- [ ] 完成路由决策引擎原型 (任何两个执行路由工作)
- [ ] Ollama 模型加载 & 推理 benchmark
- [ ] 创建第一个 HTTP API 端点

### 下周

- [ ] PII 检测器上线
- [ ] 数据脱敏流程可用
- [ ] 向量 DB 初始化成功

### 关键问题需要回答

1. **云服务商选择**: OpenAI / Claude / 私有部署?
2. **隐私法规**: 需要符合 GDPR / CCPA / 中国法规?
3. **预算约束**: 云推理成本上限?
4. **硬件资源**: GPU 内存、CPU 核心数?
5. **延迟目标**: 可接受的最大响应时间?

---

## 文档导航

| 文件 | 内容 | 用途 |
|------|------|------|
| [HarborNAS-LocalAgent-Plan.md](./HarborNAS-LocalAgent-Plan.md) | 完整架构设计 | 深度理解系统 |
| [HarborNAS-LocalAgent-Roadmap.md](./HarborNAS-LocalAgent-Roadmap.md) | 14 周实施计划 | 项目管理 & 追踪 |
| 本文件 | 快速参考指南 | 日常开发 |

---

**版本**: 1.0  
**最后更新**: 2026-03-22  
**维护者**: TBD

