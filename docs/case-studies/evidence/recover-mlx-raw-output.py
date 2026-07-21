"""Recover what the 8bit model actually emits, since mlx-lm swallows it.

Applies the same chat template with the same tools that willitcall sends, then
generates with greedy decoding and prints the raw text with no parsing.
"""
from mlx_lm import load, generate
from mlx_lm.sample_utils import make_sampler

REPO = "mlx-community/Qwen2.5-7B-Instruct-8bit"

TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the current weather for a city.",
            "parameters": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            },
        },
    }
]

MESSAGES = [{"role": "user", "content": "What is the weather in Boston?"}]

model, tokenizer = load(REPO)
prompt = tokenizer.apply_chat_template(
    MESSAGES, tools=TOOLS, add_generation_prompt=True, tokenize=False
)
sampler = make_sampler(temp=0.0)
out = generate(model, tokenizer, prompt=prompt, max_tokens=200, sampler=sampler, verbose=False)
print("---RAW BEGIN---")
print(repr(out))
print("---RAW END---")
