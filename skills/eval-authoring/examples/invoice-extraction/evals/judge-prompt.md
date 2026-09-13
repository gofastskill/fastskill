Review the extracted invoice JSON against the task and recorded workspace evidence.

Task: {{case.prompt}}
Answer: {{trial.final_answer}}
Expected facts: {{case.expected}}

{{rubric}}

Reply using this contract:
{{output_contract}}
