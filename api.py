import os
import inspect
from typing import List, Optional, Union

from fastapi import FastAPI
from pydantic import BaseModel, Field
from vllm import LLM, SamplingParams


DEFAULT_MODEL = os.environ.get("VLLM_MODEL", "meta-llama/Meta-Llama-3-8B-Instruct")

app = FastAPI(title="vLLM API", version="0.1.0")


@app.on_event("startup")
def _startup() -> None:
    # Load the model once on process startup so requests are fast.
    app.state.model_name = DEFAULT_MODEL
    app.state.llm = LLM(model=DEFAULT_MODEL)


@app.get("/health")
def health() -> dict:
    return {"status": "ok", "model": getattr(app.state, "model_name", DEFAULT_MODEL)}


class GenerateRequest(BaseModel):
    prompt: Union[str, List[str]] = Field(..., description="Prompt string or list of prompts.")

    max_tokens: int = Field(128, ge=1, description="Maximum number of generated tokens.")
    temperature: float = Field(0.7, ge=0.0, description="Sampling temperature.")
    top_p: float = Field(0.95, ge=0.0, le=1.0, description="Nucleus sampling probability.")
    stop: Optional[Union[str, List[str]]] = Field(
        default=None, description="Stop sequence(s)."
    )
    n: int = Field(1, ge=1, le=16, description="Number of completions per prompt.")
    seed: Optional[int] = Field(default=None, description="Random seed (if supported).")


class Completion(BaseModel):
    text: str
    finish_reason: Optional[str] = None


class GenerateResponse(BaseModel):
    model: str
    outputs: List[List[Completion]]


@app.post("/generate", response_model=GenerateResponse)
def generate(req: GenerateRequest) -> GenerateResponse:
    llm: LLM = app.state.llm
    model_name: str = app.state.model_name

    prompts = [req.prompt] if isinstance(req.prompt, str) else req.prompt
    stop = [req.stop] if isinstance(req.stop, str) else req.stop

    sampling_kwargs = {
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
        "top_p": req.top_p,
        "stop": stop,
        "n": req.n,
        "seed": req.seed,
    }
    allowed = set(inspect.signature(SamplingParams).parameters.keys())
    sampling = SamplingParams(
        **{k: v for k, v in sampling_kwargs.items() if k in allowed and v is not None}
    )

    results = llm.generate(prompts, sampling_params=sampling)

    outputs: List[List[Completion]] = []
    for r in results:
        completions: List[Completion] = []
        for o in r.outputs:
            completions.append(
                Completion(text=o.text, finish_reason=getattr(o, "finish_reason", None))
            )
        outputs.append(completions)

    return GenerateResponse(model=model_name, outputs=outputs)


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        "api:app",
        host=os.environ.get("HOST", "0.0.0.0"),
        port=int(os.environ.get("PORT", "8000")),
        reload=False,
    )

