import torch
from mlkb.lm.model import LM, Config, rope


def test_param_count_matches_the_design():
    assert sum(p.numel() for p in LM(Config()).parameters()) == 8_933_376


def test_shapes():
    m = LM(Config(vocab=300, d=64, heads=2, layers=2, ff=128))
    x = torch.randint(4, 300, (3, 10))
    assert m(x).shape == (3, 10, 300)


def test_rope_scores_depend_only_on_distance():
    torch.manual_seed(0)
    q, k = torch.randn(1, 1, 64), torch.randn(1, 1, 64)

    def score(m, n):
        return (rope(q, torch.tensor([m]), 10000.0) * rope(k, torch.tensor([n]), 10000.0)).sum().item()

    assert abs(score(5, 2) - score(105, 102)) < 1e-4


def test_overfits_one_sequence():
    torch.manual_seed(0)
    m = LM(Config(vocab=50, d=64, heads=2, layers=2, ff=128, dropout=0.0))
    x = torch.tensor([[2, 10, 11, 12, 13, 14, 15]])
    opt = torch.optim.Adam(m.parameters(), 3e-3)
    for _ in range(200):
        loss = torch.nn.functional.cross_entropy(m(x[:, :-1]).transpose(1, 2), x[:, 1:])
        opt.zero_grad()
        loss.backward()
        opt.step()
    assert m(x[:, :-1]).argmax(-1).tolist() == [x[0, 1:].tolist()]
