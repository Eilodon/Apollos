import asyncio
import base64
import json
import logging
from typing import Any

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from google.adk.runners import Runner
from google.adk.sessions import InMemorySessionService
from google.genai import types

from .agent.aria_agent import create_aria_agent
from .agent.run_config import get_run_config

logger = logging.getLogger(__name__)

app = FastAPI(title="VisionGPT ARIA - Pure ADK Backend")

# Khởi tạo Pure ADK
root_agent = create_aria_agent()
session_service = InMemorySessionService()
runner = Runner(
    agent=root_agent,
    session_service=session_service,
    app_name="visiongpt-aria",
)

# Lưu active WS cho HARD_STOP emit (chỉ còn /ws/live)
active_connections: dict[str, WebSocket] = {}

async def receive_loop(websocket: WebSocket, session_id: str, queue: asyncio.Queue[types.LiveClientContent]) -> None:
    """Read websocket messages and push them into ADK runner's input queue."""
    try:
        while True:
            data = await websocket.receive_json()
            msg_type = data.get("type")

            if msg_type == "audio_chunk":
                b64_audio = data.get("audio_chunk_pcm16", "")
                if b64_audio:
                    audio_bytes = base64.b64decode(b64_audio)
                    # Create LiveClientContent Realtime Input
                    content = types.LiveClientContent(
                        realtime_input=types.LiveClientRealtimeInput(
                            media_chunks=[
                                types.Blob(
                                    data=audio_bytes,
                                    mime_type="audio/pcm;rate=16000"
                                )
                            ]
                        )
                    )
                    await queue.put(content)

            elif msg_type == "multimodal_frame":
                b64_frame = data.get("frame_jpeg_base64", "")
                parts: list[Any] = []
                
                if b64_frame:
                    parts.append(
                        types.Part.from_bytes(
                            data=base64.b64decode(b64_frame),
                            mime_type="image/jpeg",
                        )
                    )
                
                motion_state = data.get("motion_state", "stationary")
                pitch = data.get("pitch", 0.0)
                vel = data.get("velocity", 0.0)
                
                text_content = f"[KINEMATIC: User is {motion_state}. Pitch: {pitch:.1f}deg. Velocity: {vel:.2f}. Treat visible hazards with safety-first urgency.]"
                parts.append(types.Part.from_text(text=text_content))

                content = types.LiveClientContent(
                    turns=[types.Content(role="user", parts=parts)],
                    turn_complete=True
                )
                await queue.put(content)
            
            elif msg_type == "user_command":
                cmd = data.get("command", "")
                if cmd:
                    content = types.LiveClientContent(
                        turns=[types.Content(role="user", parts=[types.Part.from_text(text=cmd)])],
                        turn_complete=True
                    )
                    await queue.put(content)

    except WebSocketDisconnect:
        pass
    except Exception as e:
        logger.error(f"Receive loop error for {session_id}: {e}")

async def send_loop(websocket: WebSocket, session_id: str, queue: asyncio.Queue[types.LiveClientContent]) -> None:
    """Read ADK runner events and send them out via websocket."""
    run_config = get_run_config()
    try:
        async for event in runner.run_live(
            session_id=session_id,
            run_config=run_config,
            live_request_queue=queue
        ):
            if event.type == "audio":
                await websocket.send_bytes(event.audio_data)

            elif event.type == "function_call":
                # Send out HARD_STOP if applicable
                await handle_function_call(event, websocket, session_id)
                
                # Push back function response to unblock the LLM turn
                func_responses = []
                for fc in event.function_calls or []:
                    func_responses.append(
                        types.FunctionResponse(
                            id=fc.id,
                            name=fc.name,
                            response={"status": "OK", "session_id": session_id}
                        )
                    )
                
                if func_responses:
                    content = types.LiveClientContent(
                        tool_response=types.LiveClientToolResponse(
                            function_responses=func_responses
                        )
                    )
                    await queue.put(content)

    except Exception as e:
        logger.error(f"Send loop error for {session_id}: {e}")
        try:
            await websocket.close()
        except Exception:
            pass

@app.websocket("/ws/live/{session_id}")
async def websocket_live_endpoint(websocket: WebSocket, session_id: str):
    await websocket.accept()
    active_connections[session_id] = websocket
    
    # ADK's native live input queue
    live_queue: asyncio.Queue[types.LiveClientContent] = asyncio.Queue()
    
    # Create bidirectional tasks
    recv_task = asyncio.create_task(receive_loop(websocket, session_id, live_queue))
    send_task = asyncio.create_task(send_loop(websocket, session_id, live_queue))
    
    try:
        done, pending = await asyncio.wait(
            [recv_task, send_task],
            return_when=asyncio.FIRST_COMPLETED
        )
        for t in pending:
            t.cancel()
    except Exception as e:
        logger.error(f"Endpoint error: {e}")
    finally:
        active_connections.pop(session_id, None)

async def handle_function_call(event, websocket: WebSocket, session_id: str):
    """Xử lý HARD_STOP khi Gemini gọi tool"""
    if event.tool_name == "log_hazard_event":
        payload = {
            "type": "HARD_STOP",
            "position_x": event.args.get("position_x", 0.0),
            "distance": event.args.get("distance_category", "mid"),
            "hazard_type": event.args.get("hazard_type")
        }
        await websocket.send_json(payload)   # Trigger siren ngay lập tức
