# HarborNAS 本地智能体 - 实施路线图

## 项目时间线 (14周)

```
Week 1-2        Week 3-4         Week 5-7        Week 8-9         Week 10-12       Week 13-14
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  核心框架 │    │ 隐私安全  │    │  多模态RAG  │    │ 云协作混合 │    │ 性能优化 │    │  测试文档 │
│  基础设施 │    │ PII检测  │    │  向量数据库 │    │  混合执行  │    │ 部署配置 │    │  上线准备 │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
     │                │                │               │              │              │
     ▼                ▼                ▼               ▼              ▼              ▼
   MVP Ready       Privacy Safe     RAG Ready      Cloud Ready     Prod Ready    Launch
```

---

## Phase 1: 核心框架与基础设施 (Week 1-2)

### 1.1 环境准备

**Task 1.1.1: 项目初始化**
```bash
# 创建项目结构
mkdir -p harbor-local-agent
cd harbor-local-agent

# 初始化 Python 项目
python -m venv venv
source venv/bin/activate
pip install --upgrade pip setuptools

# 初始化 Git
git init
git remote add origin <your-repo-url>

# 创建 .gitignore
echo "venv/" > .gitignore
echo "__pycache__/" >> .gitignore
echo "*.pyc" >> .gitignore
echo ".env" >> .gitignore
echo "logs/" >> .gitignore
```

**Task 1.1.2: 依赖安装**
```
requirements.txt:
  FastAPI==0.104.1
  uvicorn==0.24.0
  pydantic==2.5.0
  python-dotenv==1.0.0
  requests==2.31.0
  opencv-python==4.8.1
  torch==2.1.0
  torchvision==0.16.0
  ollama==0.0.45
  pymilvus==2.3.5
  sentence-transformers==2.2.2
  opentelemetry-api==1.21.0
  opentelemetry-sdk==1.21.0
  prometheus-client==0.19.0
  psycopg2-binary==2.9.9
  redis==5.0.1
  pydantic-settings==2.1.0
```

**Task 1.1.3: Docker 本地开发环境**
```yaml
# docker-compose.dev.yaml
version: '3.8'

services:
  ollama:
    image: ollama/ollama:latest
    ports:
      - "11434:11434"
    environment:
      - OLLAMA_MODELS=/models
    volumes:
      - ./models:/models
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:11434/api/tags"]
      interval: 10s
      timeout: 5s
      retries: 5

  milvus:
    image: milvusdb/milvus:v2.3.3
    ports:
      - "19530:19530"
      - "9091:9091"
    environment:
      ETCD_ENDPOINTS: etcd:2379
      COMMON_STORAGETYPE: local
    depends_on:
      - etcd
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9091/healthz"]
      interval: 10s
      timeout: 5s
      retries: 5

  etcd:
    image: quay.io/coreos/etcd:v3.5.5
    environment:
      - ETCD_AUTO_COMPACTION_MODE=revision
      - ETCD_AUTO_COMPACTION_RETENTION=1000
    ports:
      - "2379:2379"

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5

  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: harbor_agent
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  postgres_data:
```

### 1.2 核心代码框架

**Task 1.2.1: 路由决策引擎框架**

```python
# src/core/router.py

from enum import Enum
from dataclasses import dataclass
from typing import Literal

ExecutionRoute = Literal["LOCAL", "HYBRID", "CLOUD"]

@dataclass
class RoutingDecision:
    route: ExecutionRoute
    confidence: float
    reasoning: str
    estimated_latency_ms: int
    estimated_cost: float

class Router:
    def __init__(self, complexity_assessor, privacy_classifier, resource_manager):
        self.complexity_assessor = complexity_assessor
        self.privacy_classifier = privacy_classifier
        self.resource_manager = resource_manager
    
    async def route(self, task: dict) -> RoutingDecision:
        """
        Route task to optimal execution path
        """
        # 步骤1: 评估复杂度
        complexity_score = await self.complexity_assessor.evaluate(task)
        
        # 步骤2: 检查隐私风险
        privacy_result = await self.privacy_classifier.classify(task)
        
        # 步骤3: 评估本地资源
        local_resources = await self.resource_manager.get_available_resources()
        
        # 步骤4: 做出决策
        decision = self._make_decision(
            complexity_score=complexity_score,
            privacy_result=privacy_result,
            available_resources=local_resources
        )
        
        return decision
    
    def _make_decision(self, complexity_score, privacy_result, available_resources) -> RoutingDecision:
        """
        Decision tree:
        1. 如果检测到高敏感 PII
           - 本地可处理 → LOCAL
           - 需要云端 → 脱敏后 CLOUD
        2. 否则, 根据复杂度与资源
           - complexity < 30 → LOCAL
           - 30 <= complexity < 70 → HYBRID
           - complexity >= 70 → CLOUD or HYBRID
        """
        # TODO: 实现决策逻辑
        pass
```

**Task 1.2.2: 配置管理**

```python
# src/utils/config.py

from pydantic_settings import BaseSettings
from functools import lru_cache

class Settings(BaseSettings):
    # 应用配置
    APP_NAME: str = "HarborNAS LocalAgent"
    DEBUG: bool = True
    LOG_LEVEL: str = "INFO"
    
    # Ollama 配置
    OLLAMA_BASE_URL: str = "http://localhost:11434"
    
    # Milvus 配置
    MILVUS_HOST: str = "localhost"
    MILVUS_PORT: int = 19530
    VECTOR_DIMENSION: int = 384
    
    # Redis 配置
    REDIS_URL: str = "redis://localhost:6379"
    
    # 数据库配置
    DATABASE_URL: str = "postgresql://postgres:postgres@localhost:5432/harbor_agent"
    
    # 云 API 配置
    OPENAI_API_KEY: str = ""
    ANTHROPIC_API_KEY: str = ""
    
    # 模型配置
    LOCAL_MODELS: list = ["mistral:7b", "llama2:13b"]
    DEFAULT_LOCAL_MODEL: str = "mistral:7b"
    DEFAULT_CLOUD_MODEL: str = "gpt-4"
    
    class Config:
        env_file = ".env"
        env_file_encoding = "utf-8"

@lru_cache()
def get_settings() -> Settings:
    return Settings()
```

**Task 1.2.3: API 框架**

```python
# src/api/router.py

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel
from uuid import uuid4

router = APIRouter(prefix="/api/v1")

class TaskRequest(BaseModel):
    query: str
    metadata: dict = {}
    prefer_local: bool = True

class TaskResponse(BaseModel):
    task_id: str
    status: str
    route: str
    result: str = None
    latency_ms: float

@router.post("/tasks")
async def create_task(request: TaskRequest):
    """
    接收任务请求，触发路由决策
    """
    task_id = str(uuid4())
    
    # 1. 解析请求
    task = {
        'id': task_id,
        'query': request.query,
        'metadata': request.metadata,
        'prefer_local': request.prefer_local
    }
    
    # 2. 路由
    # TODO: 调用 router
    
    # 3. 入队处理
    # TODO: 推送到消息队列
    
    return {
        'task_id': task_id,
        'status': 'queued'
    }

@router.get("/tasks/{task_id}")
async def get_task_status(task_id: str):
    """
    查询任务状态和结果
    """
    # TODO: 从数据库查询
    pass
```

### 1.3 监控基础设施

**Task 1.3.1: 日志系统**

```python
# src/monitoring/logger.py

import logging
import json
from datetime import datetime
import sys

class JSONFormatter(logging.Formatter):
    def format(self, record):
        log_entry = {
            'timestamp': datetime.utcnow().isoformat(),
            'level': record.levelname,
            'logger': record.name,
            'message': record.getMessage(),
            'module': record.module,
            'function': record.funcName,
            'line': record.lineno
        }
        return json.dumps(log_entry)

def setup_logger(name: str) -> logging.Logger:
    logger = logging.getLogger(name)
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(JSONFormatter())
    logger.addHandler(handler)
    return logger
```

**Task 1.3.2: 指标收集**

```python
# src/monitoring/metrics.py

from prometheus_client import Counter, Histogram, Gauge

# 路由指标
routing_counter = Counter(
    'routing_decisions_total',
    'Total routing decisions',
    ['route_type', 'status']
)

local_latency = Histogram(
    'local_execution_latency_ms',
    'Local execution latency',
    buckets=[100, 200, 500, 1000]
)

cloud_latency = Histogram(
    'cloud_execution_latency_ms',
    'Cloud execution latency',
    buckets=[1000, 2000, 5000, 10000]
)

local_ratio_gauge = Gauge(
    'execution_local_ratio',
    'Ratio of local executions'
)
```

---

## Phase 2: 隐私与安全 (Week 3-4)

### 2.1 PII 检测

**Task 2.1.1: PII 检测器实现**

```python
# src/security/pii_detector.py

import re
from typing import Dict, List, Tuple

class PIIDetector:
    def __init__(self):
        self.patterns = {
            'email': r'\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b',
            'phone': r'\b(?:\+?1[-.]?)?(?:\([-.]?\d{3}[-.]?\)|\d{3})[-.]?\d{3}[-.]?\d{4}\b',
            'ssn': r'\b\d{3}-\d{2}-\d{4}\b',
            'credit_card': r'\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13})\b',
            'url': r'https?://[^\s]+',
            'ipv4': r'\b(?:\d{1,3}\.){3}\d{1,3}\b',
            'chinese_id': r'\d{18}|\d{17}[X]',
        }
        
        self.detectors = {
            'email': self._detect_email,
            'phone': self._detect_phone,
            # ... 更多检测器
        }
    
    def detect(self, text: str) -> Dict[str, List[str]]:
        """
        Check if PII exists in text
        Returns: {'pii_type': [matches], ...}
        """
        pii_found = {}
        
        for pii_type, pattern in self.patterns.items():
            matches = re.findall(pattern, text)
            if matches:
                pii_found[pii_type] = matches
        
        return pii_found
    
    def _detect_email(self, text: str) -> List[str]:
        # 可视化正则后处理, 减少误报
        matches = re.findall(self.patterns['email'], text)
        return [m for m in matches if self._is_valid_email(m)]
    
    def _is_valid_email(self, email: str) -> bool:
        # 检查域名是否真实
        # TODO: 可选的 DNS 检查
        pass
```

### 2.2 数据脱敏

**Task 2.2.1: 脱敏器实现**

```python
# src/security/anonymizer.py

import hashlib
from cryptography.fernet import Fernet
from typing import Dict, Tuple

class DataAnonymizer:
    def __init__(self, pii_detector):
        self.pii_detector = pii_detector
        # 本地密钥存储 (需要安全管理)
        self.cipher = Fernet(Fernet.generate_key())
        self.mapping_store = {}  # PII → Placeholder 映射
    
    def anonymize(self, text: str) -> Tuple[str, str]:
        """
        Anonymize PII in text
        Returns: (anonymized_text, encryption_key)
        """
        pii_found = self.pii_detector.detect(text)
        anonymized = text
        mapping = {}
        
        for pii_type, matches in pii_found.items():
            for original_value in matches:
                placeholder = self._generate_placeholder(pii_type)
                anonymized = anonymized.replace(original_value, placeholder)
                mapping[placeholder] = original_value
        
        # 加密映射并存储
        encrypted_mapping = self.cipher.encrypt(
            str(mapping).encode()
        )
        
        return anonymized, encrypted_mapping.decode()
    
    def deanonymize(self, text: str, encrypted_mapping: str) -> str:
        """
        恢复脱敏数据
        """
        decrypted = self.cipher.decrypt(encrypted_mapping.encode())
        mapping = eval(decrypted.decode())
        
        result = text
        for placeholder, original in mapping.items():
            result = result.replace(placeholder, original)
        
        return result
    
    def _generate_placeholder(self, pii_type: str) -> str:
        hash_val = hashlib.md5(str(pii_type).encode()).hexdigest()[:8]
        return f"[{pii_type.upper()}_{hash_val}]"
```

### 2.3 审计日志

**Task 2.3.1: 审计日志存储**

```python
# src/security/audit_logger.py

from sqlalchemy import create_engine, Column, String, DateTime, Text
from datetime import datetime
import json

class AuditLog:
    def __init__(self, database_url: str):
        self.engine = create_engine(database_url)
        self._init_tables()
    
    def _init_tables(self):
        # SQL: 创建审计日志表
        with self.engine.connect() as conn:
            conn.execute("""
            CREATE TABLE IF NOT EXISTS audit_logs (
                id UUID PRIMARY KEY,
                task_id UUID,
                timestamp TIMESTAMP,
                action VARCHAR(50),
                entity_type VARCHAR(50),
                details JSON,
                actor VARCHAR(100),
                status VARCHAR(20),
                error_message TEXT,
                created_at TIMESTAMP DEFAULT NOW()
            );
            
            CREATE INDEX idx_audit_task ON audit_logs(task_id);
            CREATE INDEX idx_audit_timestamp ON audit_logs(timestamp);
            """)
            conn.commit()
    
    def log_action(self, action: str, details: dict):
        """
        Log security-relevant action
        """
        # 插入审计日志
        # TODO: 实现数据库插入逻辑
        pass
```

---

## Phase 3: 多模态 RAG 系统 (Week 5-7)

### 3.1 向量数据库集成

**Task 3.1.1: Milvus 连接与初始化**

```python
# src/rag/vector_store.py

from pymilvus import connections, Collection, CollectionSchema, FieldSchema, DataType
from typing import List
import numpy as np

class MilvusVectorStore:
    def __init__(self, host: str = "localhost", port: int = 19530):
        connections.connect(
            alias="default",
            host=host,
            port=port
        )
        self.collection = None
    
    def init_collection(self, 
                       collection_name: str,
                       dim: int = 384):
        """
        Initialize vector collection
        """
        fields = [
            FieldSchema(
                name="id",
                dtype=DataType.INT64,
                is_primary=True,
                auto_id=True
            ),
            FieldSchema(
                name="embedding",
                dtype=DataType.FLOAT_VECTOR,
                dim=dim
            ),
            FieldSchema(
                name="text",
                dtype=DataType.VARCHAR,
                max_length=65535
            ),
            FieldSchema(
                name="metadata",
                dtype=DataType.VARCHAR,
                max_length=65535
            ),
            FieldSchema(
                name="namespace",
                dtype=DataType.VARCHAR,
                max_length=50
            ),
            FieldSchema(
                name="created_at",
                dtype=DataType.INT64
            )
        ]
        
        schema = CollectionSchema(fields)
        self.collection = Collection(
            name=collection_name,
            schema=schema
        )
        self.collection.create_index(
            field_name="embedding",
            index_params={"metric_type": "L2"}
        )
        return self.collection
    
    async def insert(self, 
                   embeddings: List[List[float]],
                   texts: List[str],
                   metadata: List[dict]):
        """
        Insert vectors with metadata
        """
        # TODO: 批量插入
        pass
    
    async def search(self, 
                    query_embedding: List[float],
                    top_k: int = 10,
                    namespace: str = None) -> List[dict]:
        """
        Semantic search
        """
        # TODO: 实现搜索
        pass
```

### 3.2 文本数据摄入

**Task 3.2.1: 文本分块与向量化**

```python
# src/rag/text_ingester.py

from typing import List
from langchain.text_splitter import RecursiveCharacterTextSplitter
from sentence_transformers import SentenceTransformer

class TextIngester:
    def __init__(self, embedding_model_name: str = "all-MiniLM-L6-v2"):
        self.splitter = RecursiveCharacterTextSplitter(
            chunk_size=512,
            chunk_overlap=50
        )
        self.encoder = SentenceTransformer(embedding_model_name)
    
    async def ingest_file(self, file_path: str) -> List[dict]:
        """
        Ingest text file: read → split → embed → return
        """
        # 1. 读取文件
        with open(file_path, 'r', encoding='utf-8') as f:
            text = f.read()
        
        # 2. 分块
        chunks = self.splitter.split_text(text)
        
        # 3. 向量化
        embeddings = self.encoder.encode(chunks)
        
        # 4. 返回结构
        return [
            {
                'chunk': chunk,
                'embedding': embedding.tolist(),
                'source': file_path
            }
            for chunk, embedding in zip(chunks, embeddings)
        ]
```

### 3.3 图像摄入

**Task 3.3.1: CLIP 图像理解**

```python
# src/rag/image_ingester.py

import cv2
import numpy as np
from PIL import Image
import torch
from transformers import CLIPProcessor, CLIPModel
from typing import Tuple

class ImageIngester:
    def __init__(self, model_name: str = "openai/clip-vit-base-patch32"):
        self.model = CLIPModel.from_pretrained(model_name)
        self.processor = CLIPProcessor.from_pretrained(model_name)
    
    async def ingest_image(self, image_path: str) -> dict:
        """
        Process image: read → embed → extract metadata
        """
        # 1. 读取图像
        image = Image.open(image_path)
        
        # 2. 提取文本描述 (使用 CLIP 文到图)
        description = await self._generate_description(image)
        
        # 3. 获取图像向量
        inputs = self.processor(images=image, return_tensors="pt")
        with torch.no_grad():
            image_embedding = self.model.get_image_features(**inputs)
        
        # 4. 对象检测 (可选, 使用 YOLO)
        objects = await self._detect_objects(image_path)
        
        return {
            'image_path': image_path,
            'embedding': image_embedding.numpy().flatten().tolist(),
            'description': description,
            'objects': objects,
            'size': image.size,
            'format': image.format
        }
    
    async def _generate_description(self, image: Image) -> str:
        """
        Generate natural language description of image
        可用 template prompts 或调用 VLM
        """
        # 简单实现: 使用预定义提示词
        prompts = [
            "a photo of",
            "a scene of",
            "an image of"
        ]
        
        # TODO: 调用 CLIP 文本编码器
        # 或使用更强大的 VLM (如 GPT-4V)
        pass
    
    async def _detect_objects(self, image_path: str) -> list:
        """
        Detect objects in image using YOLO
        """
        # TODO: YOLO 对象检测
        pass
```

### 3.4 音频摄入

**Task 3.4.1: Whisper 语音识别**

```python
# src/rag/audio_ingester.py

import whisper
from pathlib import Path
from typing import Dict

class AudioIngester:
    def __init__(self, model_size: str = "base"):
        # base: ~140M, small: ~244M, medium: ~769M
        self.model = whisper.load_model(model_size)
    
    async def ingest_audio(self, audio_path: str) -> Dict:
        """
        Process audio: transcribe → embed → extract features
        """
        # 1. 转录
        result = self.model.transcribe(audio_path)
        transcription = result["text"]
        
        # 2. 分块与向量化
        from src.rag.text_ingester import TextIngester
        text_ingester = TextIngester()
        text_chunks = await text_ingester.ingest_file(
            text=transcription
        )
        
        # 3. 提取时间信息
        segments = result.get("segments", [])
        
        return {
            'audio_path': audio_path,
            'transcription': transcription,
            'segments': segments,
            'embeddings': text_chunks,
            'language': result.get('language', 'unknown')
        }
```

### 3.5 视频摄入

**Task 3.5.1: 视频关键帧提取**

```python
# src/rag/video_ingester.py

import cv2
from typing import List, Dict
import numpy as np

class VideoIngester:
    def __init__(self, fps_sample: int = 1):
        self.fps_sample = fps_sample
    
    async def ingest_video(self, video_path: str) -> Dict:
        """
        Process video: extract frames → transcribe audio → embed
        """
        cap = cv2.VideoCapture(video_path)
        fps = cap.get(cv2.CAP_PROP_FPS)
        total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        duration_sec = total_frames / fps
        
        frames = []
        timestamps = []
        frame_idx = 0
        
        # 1. 提取关键帧 (每秒采样 N 帧)
        key_frames_interval = int(fps / self.fps_sample)
        
        while True:
            ret, frame = cap.read()
            if not ret:
                break
            
            if frame_idx % key_frames_interval == 0:
                timestamp = frame_idx / fps
                frames.append(frame)
                timestamps.append(timestamp)
            
            frame_idx += 1
        
        cap.release()
        
        # 2. 图像向量化
        image_ingester = ImageIngester()
        frame_embeddings = []
        for frame in frames:
            # 转 PIL 格式
            frame_rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            pil_image = Image.fromarray(frame_rgb)
            
            # 获取嵌入
            embedding = await image_ingester._embed_image(pil_image)
            frame_embeddings.append(embedding)
        
        # 3. 音频转录
        audio_path = self._extract_audio(video_path)
        audio_ingester = AudioIngester()
        audio_data = await audio_ingester.ingest_audio(audio_path)
        
        return {
            'video_path': video_path,
            'duration_sec': duration_sec,
            'fps': fps,
            'key_frames': {
                'timestamps': timestamps,
                'embeddings': frame_embeddings
            },
            'audio': audio_data
        }
    
    def _extract_audio(self, video_path: str) -> str:
        """
        Extract audio track from video
        """
        # TODO: 使用 ffmpeg
        pass
```

---

## Phase 4: 云协作与混合执行 (Week 8-9)

### 4.1 混合执行器

**Task 4.1.1: 混合执行逻辑**

```python
# src/executors/hybrid_executor.py

from typing import Dict, Optional

class HybridExecutor:
    def __init__(self, 
                 local_executor,
                 cloud_executor,
                 anonymizer,
                 router):
        self.local_executor = local_executor
        self.cloud_executor = cloud_executor
        self.anonymizer = anonymizer
        self.router = router
    
    async def execute(self, task: Dict) -> Dict:
        """
        Mixed execution:
        1. Local preprocessing
        2. Cloud processing (if needed)
        3. Local postprocessing
        """
        task_id = task['id']
        
        # 步骤1: 本地预处理
        preprocessed = await self.local_executor.preprocess(task)
        
        # 步骤2: 评估是否需要云端
        if self._should_call_cloud(preprocessed):
            # 步骤3: 脱敏
            anonymized_data, mapping_key = self.anonymizer.anonymize(
                preprocessed['content']
            )
            
            try:
                # 步骤4: 调用云端
                cloud_result = await self.cloud_executor.inference(
                    anonymized_data,
                    task_type=task['type'],
                    context=preprocessed.get('context')
                )
                
                # 步骤5: 反脱敏
                result = self.anonymizer.deanonymize(
                    cloud_result,
                    mapping_key
                )
                
            except Exception as e:
                # 云端失败, 降级到本地
                result = await self.local_executor.execute(preprocessed)
        
        else:
            # 完全本地处理
            result = await self.local_executor.execute(preprocessed)
        
        # 步骤6: 本地后处理
        final_result = await self.local_executor.postprocess(result)
        
        return {
            'task_id': task_id,
            'result': final_result,
            'route': 'HYBRID',
            'timestamp': datetime.now().isoformat()
        }
    
    def _should_call_cloud(self, preprocessed: Dict) -> bool:
        """
        Determine if cloud invocation is necessary
        """
        complexity = preprocessed.get('estimated_complexity', 0)
        return complexity > 0.5  # 阈值可配置
```

---

## Phase 5: 性能优化 (Week 10-12)

### 5.1 模型量化

**Task 5.1.1: Ollama 模型量化**

```bash
# 下载并量化模型
ollama pull mistral:7b

# 手动量化 (如需更激进的量化)
# TODO: 使用 llama.cpp 进行 4-bit 量化
```

### 5.2 缓存策略

**Task 5.2.1: 查询缓存**

```python
# src/core/cache_manager.py

from redis import Redis
import json
from hashlib import sha256

class CacheManager:
    def __init__(self, redis_url: str):
        self.redis = Redis.from_url(redis_url, decode_responses=True)
        self.ttl = 3600  # 1 小时
    
    def get_cache_key(self, query: str) -> str:
        return f"query:{sha256(query.encode()).hexdigest()[:16]}"
    
    async def get(self, query: str) -> Optional[Dict]:
        key = self.get_cache_key(query)
        cached = self.redis.get(key)
        if cached:
            return json.loads(cached)
        return None
    
    async def set(self, query: str, result: Dict):
        key = self.get_cache_key(query)
        self.redis.setex(
            key,
            self.ttl,
            json.dumps(result)
        )
```

---

## Phase 6: 测试与文档 (Week 13-14)

### 6.1 单元测试框架

**Task 6.1.1: 测试结构**

```python
# tests/unit/test_router.py

import pytest
from src.core.router import Router

@pytest.fixture
def router():
    # 初始化 mock 依赖
    return Router(...)

@pytest.mark.asyncio
async def test_simple_task_routes_to_local(router):
    """Simple tasks should route to LOCAL"""
    task = {'query': 'what is 2+2?', 'complexity': 10}
    decision = await router.route(task)
    assert decision.route == "LOCAL"

@pytest.mark.asyncio
async def test_complex_task_routes_to_cloud(router):
    """Complex tasks should route to CLOUD"""
    task = {'query': '...complex task...', 'complexity': 90}
    decision = await router.route(task)
    assert decision.route == "CLOUD"

@pytest.mark.asyncio
async def test_pii_detected_triggers_anonymization(router):
    """Tasks with PII should trigger anonymization"""
    task = {'query': 'My SSN is 123-45-6789'}
    decision = await router.route(task)
    assert decision.pii_detected
```

### 6.2 集成测试

**Task 6.2.1: E2E 流程测试**

```python
# tests/integration/test_end_to_end.py

@pytest.mark.asyncio
async def test_full_pipeline(client, ollama_running, milvus_running):
    """Test complete task pipeline"""
    
    # 1. 提交任务
    response = await client.post(
        "/api/v1/tasks",
        json={"query": "Search my photos with people"}
    )
    task_id = response.json()['task_id']
    
    # 2. 等待处理
    for _ in range(30):  # 30 秒超时
        response = await client.get(f"/api/v1/tasks/{task_id}")
        if response.json()['status'] == 'completed':
            break
        await asyncio.sleep(1)
    
    # 3. 验证结果
    result = response.json()
    assert result['status'] == 'completed'
    assert len(result['results']) > 0
```

---

## 关键工作清单

### 立即行动 (Week 0)

- [ ] 创建 GitHub 仓库并初始化项目结构
- [ ] 设置开发环境 (Docker Compose)
- [ ] 确认技术选型 (模型、向量DB、云服务商)
- [ ] 发起团队讨论会 (架构评审)

### Week 1-2 优先级

- [ ] ✅ 环境初始化 & Docker 搭建
- [ ] ✅ 路由决策引擎原型
- [ ] ✅ Ollama 集成测试
- [ ] ✅ 第一个 API 端点可用

### 关键风险

| 风险 | 缓解 |
|------|-----|
| 本地推理性能不足 | 提前 benchmark (目标 <500ms) |
| 脱敏流程遗漏 PII | 多层检测 + 审计日志 |
| 向量检索准确度低 | 混合检索 (向量+BM25+重排) |
| 云 API 成本超预算 | 严格的路由策略 + 配额限制 |

### 成功指标

- Phase 1 完成: MVP 能执行本地推理
- Phase 2 完成: PII 检测准确率 > 99%
- Phase 3 完成: 多模态检索 NDCG@10 > 85%
- Phase 4 完成: 混合执行成功率 > 95%
- Phase 5 完成: 本地推理延迟 < 500ms
- Phase 6 完成: 测试覆盖率 > 80%, 可上线

