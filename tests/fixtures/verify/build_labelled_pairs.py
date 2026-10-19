"""Build the labelled claim/evidence set the verifier benchmark scores.

Usage (repo root): python3 -I tests/fixtures/verify/build_labelled_pairs.py

Evidence is never typed by hand: every pair names a sentence by its number
in the recorded abstracts (tests/fixtures/http/expected.json, recorded
cases, split into sentences exactly as below), and the script copies that
sentence verbatim. Claims and labels were written by hand:

- supported: the sentence entails the claim;
- contradicted: the sentence states the opposite (negation, reversal, or a
  changed number or entity);
- neutral: on topic, but the sentence neither entails nor contradicts it.

Labeller: Claude (AI), for the Rust port's Phase 5; not yet reviewed by a
person. Treat scores on it as indicative, not as a published result.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).with_name("labelled_pairs.json")

S, C, N = "supported", "contradicted", "neutral"

# (sentence number, claim, label)
PAIRS: list[tuple[int, str, str]] = [
    (1, "BNRM integrates non-negative factor analysis into the Bradley-Terry preference model.", S),
    (1, "BNRM removes the Bradley-Terry preference model entirely.", C),
    (1, "BNRM was trained on more than one trillion tokens.", N),
    (3, "An amortized variational inference network lets BNRM scale to modern LLMs.", S),
    (3, "BNRM cannot be trained end to end.", C),
    (4, "BNRM substantially mitigates reward over-optimization.", S),
    (4, "BNRM makes reward over-optimization substantially worse.", C),
    (4, "BNRM was evaluated on medical question answering.", N),
    (5, "RLHF is the primary method for aligning large language models with human preferences.", S),
    (5, "RLHF has been abandoned as a way to align large language models.", C),
    (
        6,
        "The RLHF process typically starts by training a reward model on human preference data.",
        S,
    ),
    (6, "The RLHF process typically starts without any reward model.", C),
    (7, "Conventional reward models are trained on pairwise responses to the same request.", S),
    (7, "Conventional reward models are trained on single responses with absolute scores only.", C),
    (8, "Reward model outputs lack interpretability because reward models are black boxes.", S),
    (8, "Humans can easily understand why a reward model prefers a response.", C),
    (8, "Interpretable reward models are cheaper to train.", N),
    (10, "ArmoRM was trained with Llama-3 8B and a gating network made of a shallow MLP.", S),
    (10, "ArmoRM was trained with a 70B model and no gating network.", C),
    (10, "ArmoRM was trained in under one hour.", N),
    (
        12,
        "Specification gaming occurs when AI systems learn highly rewarded but undesired behaviours.",
        S,
    ),
    (12, "Specification gaming only happens when training goals are perfectly specified.", C),
    (13, "Reward-tampering means a model directly modifies its own reward mechanism.", S),
    (13, "Sycophancy is the most dangerous form of specification gaming.", N),
    (
        16,
        "Training on early-curriculum environments leads to more specification gaming on the remaining environments.",
        S,
    ),
    (
        16,
        "Training on early-curriculum environments eliminates specification gaming on later environments.",
        C,
    ),
    (
        17,
        "LLM assistants trained on the full curriculum sometimes rewrite their own reward function zero-shot.",
        S,
    ),
    (
        17,
        "LLM assistants trained on the full curriculum never rewrite their own reward function.",
        C,
    ),
    (
        18,
        "Retraining an LLM not to game early environments mitigates reward-tampering later on.",
        S,
    ),
    (
        18,
        "Retraining an LLM not to game early environments completely eliminates reward-tampering.",
        C,
    ),
    (
        19,
        "Adding harmlessness training to the gameable environments does not prevent reward-tampering.",
        S,
    ),
    (19, "Harmlessness training prevents reward-tampering in the gameable environments.", C),
    (19, "Harmlessness training reduces sycophancy by half.", N),
    (
        21,
        "An effective energy management strategy is essential to optimise the energy efficiency of electric vehicles.",
        S,
    ),
    (
        21,
        "Energy management strategy has no effect on the energy efficiency of electric vehicles.",
        C,
    ),
    (24, "LearningEMS supports hybrid, fuel cell and plug-in electric vehicles.", S),
    (24, "LearningEMS supports only one type of electric vehicle.", C),
    (24, "LearningEMS was developed by a car manufacturer.", N),
    (
        25,
        "LearningEMS compares imitation learning, deep reinforcement learning, offline RL, model predictive control and dynamic programming.",
        S,
    ),
    (25, "LearningEMS compares only rule-based energy management strategies.", C),
    (28, "The framework includes over 160 benchmark tasks.", S),
    (28, "The framework includes fewer than ten benchmark tasks.", C),
    (28, "The framework's data set was collected in Europe.", N),
    (
        30,
        "Compulsive behaviour has been proposed as a transdiagnostic trait across neuropsychiatric disorders.",
        S,
    ),
    (
        30,
        "Compulsive behaviour is unique to obsessive-compulsive disorder and is never seen in autism.",
        C,
    ),
    (30, "Compulsive behaviour is caused by a single gene.", N),
    (33, "High-drinker animals performed more perseverative responses under punishment.", S),
    (33, "High-drinker animals performed fewer perseverative responses under punishment.", C),
    (35, "Whether retirement is desirable may depend on pre-retirement work characteristics.", S),
    (
        35,
        "Pre-retirement work characteristics are irrelevant to whether retirement is desirable.",
        C,
    ),
    (
        37,
        "The study included 1735 respondents from the Swedish Longitudinal Occupational Survey of Health.",
        S,
    ),
    (37, "The study included 17,350 respondents.", C),
    (37, "Most respondents retired because of illness.", N),
    (37, "The mean retirement age in the sample was 66 years.", S),
    (37, "The mean retirement age in the sample was 58 years.", C),
    (
        39,
        "Group-based trajectory modelling was used to model depressive symptoms across retirement.",
        S,
    ),
    (39, "Depressive symptoms were measured with brain imaging.", N),
    (41, "In four trajectories, depressive symptoms decreased slightly around retirement.", S),
    (41, "In four trajectories, depressive symptoms rose sharply around retirement.", C),
    (44, "The medical model reached 77.0% accuracy on MedMCQA.", S),
    (44, "The medical model reached 97% accuracy on MedMCQA.", C),
    (44, "The medical model outperforms human doctors on MedMCQA.", N),
    (45, "Adolescence is an age of risk for the emergence of depression.", S),
    (45, "Adolescence is a period of especially low risk for depression.", C),
    (
        47,
        "Recent evidence suggests bidirectional associations between inflammation and reward responsiveness.",
        S,
    ),
    (47, "Inflammation and reward responsiveness are unrelated.", C),
    (47, "Anti-inflammatory drugs cure adolescent depression.", N),
    (49, "Best-of-N sampling relies on reward models trained with pairwise comparison data.", S),
    (49, "Best-of-N sampling does not use reward models.", C),
    (50, "Pairwise preference training fails to capture whether a response is acceptable.", S),
    (50, "Pairwise preference training reliably captures whether a response is acceptable.", C),
    (51, "The risk of false acceptances increases with the number of samples on hard prompts.", S),
    (51, "The risk of false acceptances decreases as more samples are drawn on hard prompts.", C),
    (55, "Used as an alignment guardrail, the method reduces reliability failures by 70%.", S),
    (55, "Used as an alignment guardrail, the method increases reliability failures by 70%.", C),
    (55, "The method was tested on medical images.", N),
    (
        55,
        "Used as an inference accelerator, the method improves average inference speed by over 22%.",
        S,
    ),
    (57, "MRMC is a model checker for discrete-time and continuous-time Markov reward models.", S),
    (57, "MRMC is a neural network for language modelling.", C),
    (57, "MRMC is written in Java.", N),
    (58, "MRMC supports reward extensions of PCTL and CSL.", S),
    (60, "MRMC includes several numerical algorithms.", S),
    (60, "MRMC was released under a commercial licence.", N),
    (
        63,
        "The paper introduces a new parameterisation of the reward model in RLHF that yields the optimal policy in closed form.",
        S,
    ),
    (
        63,
        "Solving the RLHF problem still requires reinforcement learning with this parameterisation.",
        C,
    ),
    (64, "DPO is stable, performant and computationally lightweight.", S),
    (64, "DPO requires sampling from the language model during fine-tuning.", C),
    (64, "DPO was invented at a university in Europe.", N),
    (
        65,
        "DPO can align language models with human preferences as well as or better than existing methods.",
        S,
    ),
    (65, "DPO aligns language models with human preferences much worse than existing methods.", C),
    (
        67,
        "In RLHF it is common to optimise against a reward model trained to predict human preferences.",
        S,
    ),
    (67, "Reward models in RLHF usually cost more to train than the policy.", N),
    (68, "Optimising a proxy reward model too much can hinder ground-truth performance.", S),
    (68, "Optimising a proxy reward model more always improves ground-truth performance.", C),
    (68, "Goodhart's law was first stated by an economist.", N),
    (70, "A fixed gold-standard reward model plays the role of humans in the synthetic setup.", S),
    (70, "Real human annotators labelled every sample in the study.", C),
    (72, "The coefficients scale smoothly with the number of reward model parameters.", S),
    (72, "The relationship has the same functional form for every optimisation method.", C),
    (74, "The authors explore implications of their results for AI alignment theory.", S),
    (77, "Simpler direct alignment algorithms are reported to work better in many cases.", S),
    (77, "Reward models used in alignment are usually smaller than one million parameters.", N),
    (78, "RewardBench 2 uses new human prompts instead of prompts from downstream evaluations.", S),
    (78, "RewardBench 2 reuses existing prompts from downstream evaluations.", C),
    (78, "RewardBench 2 was released in 2021.", N),
    (76, "Some reward model benchmarks test agreement with human preferences.", S),
    (
        75,
        "Reward models are used in post-training across instruction following, reasoning and safety.",
        S,
    ),
    (75, "Reward models are only used during pre-training.", C),
    (75, "Reward models are mostly trained on code.", N),
    (14, "Pernicious behaviours may be too complex to discover via exploration.", S),
    (
        22,
        "Interest in sophisticated energy management for electric vehicles is growing with machine learning.",
        S,
    ),
    (22, "Electric vehicles will replace all petrol cars by 2030.", N),
    (
        26,
        "The algorithms were evaluated on energy efficiency, consistency, adaptability and practicability.",
        S,
    ),
    (26, "The algorithms were evaluated on passenger comfort.", N),
    (36, "The study examined depressive symptom trajectories across retirement.", S),
    (36, "Retirement was found to cause dementia.", N),
    (
        53,
        "The new reward model can distinguish responses that are good enough, not just better.",
        S,
    ),
    (
        54,
        "Best of mini-N in-loop splits the generation budget into sequential loops with early exit.",
        S,
    ),
    (54, "Best of mini-N in-loop requires twice the generation budget of standard best-of-N.", N),
    (
        62,
        "Existing steerability methods collect human labels of the relative quality of generations.",
        S,
    ),
    (61, "Unsupervised language models are trivially easy to control precisely.", C),
    (61, "Large unsupervised language models learn broad world knowledge.", S),
    (
        71,
        "The study optimises against the proxy reward model with reinforcement learning or best-of-n sampling.",
        S,
    ),
    (71, "The proxy reward model was trained on images.", N),
    (73, "The KL penalty coefficient was one of the factors studied.", S),
    (73, "The KL penalty was removed from the reinforcement learning setup.", C),
    (73, "The best KL coefficient was exactly 0.1.", N),
    (
        9,
        "The authors argue reward models should be human-interpretable to help prevent reward hacking.",
        S,
    ),
    (9, "The authors argue interpretability is irrelevant to reward hacking.", C),
    (20, "LLMs can generalise from common specification gaming to reward tampering.", S),
    (20, "Reward tampering behaviour is trivial to remove from LLMs.", C),
    (20, "Reward tampering was observed in production chat assistants.", N),
    (11, "ArmoRM-Llama3-8B obtains state-of-the-art performance on RewardBench.", S),
    (11, "ArmoRM-Llama3-8B performs worst among models on RewardBench.", C),
    (34, "Previous evidence on retirement's mental health effects has been inconclusive.", S),
    (34, "Previous evidence conclusively shows retirement improves mental health.", C),
    (46, "There are major unanswered questions about why depression surges in adolescence.", S),
    (46, "The causes of the adolescent rise in depression are fully understood.", C),
    (
        48,
        "The authors discuss implications of the immuno-reward model for public health policy.",
        S,
    ),
    (48, "The immuno-reward model has already been adopted as national policy.", N),
    (58, "MRMC can verify properties about cumulative rewards.", S),
    (58, "MRMC cannot verify any reward-related property.", C),
    (
        59,
        "MRMC checks reachability of goal states under time and accumulated reward constraints.",
        S,
    ),
    (66, "DPO exceeds PPO-based RLHF in controlling the sentiment of generations.", S),
    (66, "PPO-based RLHF controls sentiment better than DPO.", C),
    (
        69,
        "Measuring this effect carefully is expensive because human preference data is costly.",
        S,
    ),
    (69, "Human preference data is cheap and easy to collect.", C),
]


def sentences() -> dict[int, tuple[str, str, str]]:
    """Sentence number -> (source, title, sentence), as numbered above."""
    data = json.loads((ROOT / "tests/fixtures/http/expected.json").read_text(encoding="utf-8"))
    seen: set[str] = set()
    out: dict[int, tuple[str, str, str]] = {}
    for key, case in sorted(data["cases"].items()):
        if not key.endswith("__recorded"):
            continue
        for evidence in case["evidence"]:
            text = " ".join(re.sub(r"<[^>]+>", " ", evidence["snippet"]).split())
            for sentence in re.split(r"(?<=[.!?])\s+", text):
                if 60 < len(sentence) < 260 and sentence not in seen:
                    seen.add(sentence)
                    out[len(out) + 1] = (key.split("__")[0], evidence["title"], sentence)
    return out


def main() -> None:
    corpus = sentences()
    pairs = []
    for index, (number, claim, label) in enumerate(PAIRS, start=1):
        source, title, evidence = corpus[number]
        pairs.append(
            {
                "id": f"pair_{index:03d}",
                "claim": claim,
                "evidence": evidence,
                "label": label,
                "source": source,
                "title": title,
            }
        )
    counts = {label: sum(p["label"] == label for p in pairs) for label in (S, C, N)}
    payload = {
        "description": __doc__.strip().splitlines()[0],
        "labeller": "Claude (AI); not yet reviewed by a person",
        "counts": counts,
        "pairs": pairs,
    }
    OUT.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {len(pairs)} pairs {counts}")


if __name__ == "__main__":
    main()
