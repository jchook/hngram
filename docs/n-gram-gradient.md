Yes. If we condition on **being observed at least once** and look at nonsense n-grams for (n=1,\dots,5), a clean way to model the decline is with a **survival probability per added token**.

Let

* (X_n) = number of global occurrences of a particular nonsense (n)-gram
* and we care about (E[X_n \mid X_n \ge 1])

For nonsense strings, each extra token usually multiplies the chance of continuing by a factor much smaller than 1.

## A simple recurrence

Write

[
E[X_{n+1}\mid X_{n+1}\ge1] \approx \rho_n , E[X_n\mid X_n\ge1]
]

with (0<\rho_n<1).

If the decay is roughly stationary over this range, use a constant (\rho):

[
E[X_n\mid X_n\ge1] \approx A \rho^{,n-1}
]

where (A = E[X_1\mid X_1\ge1]).

Then the expected discrete gradient is

[
\Delta_n
= E[X_{n+1}\mid X_{n+1}\ge1] - E[X_n\mid X_n\ge1]
\approx A\rho^{n-1}(\rho-1)
]

Since (\rho<1), this is negative.

---

## Interpretation

This says:

* from 1→2, you lose a fraction (1-\rho)
* from 2→3, you lose the same fraction again
* and so on

So the slope is steep early, then flattens in absolute size as the conditional expectation gets smaller.

---

## More explicit probabilistic derivation

Suppose a nonsense (n)-gram appears according to a Poisson rate (\lambda_n), with

[
\lambda_n = C q^n
]

where

* (C) reflects corpus size
* (q) is the effective probability of extending the nonsense sequence by one more token

Then for Poisson counts,

[
E[X_n \mid X_n \ge 1]
=====================

\frac{\lambda_n}{1-e^{-\lambda_n}}
]

So

[
E[X_n \mid X_n \ge 1]
=====================

\frac{Cq^n}{1-e^{-Cq^n}}
]

This is a nicer formula because it handles the conditioning correctly.

### Two regimes

When (\lambda_n \gg 1):

[
E[X_n\mid X_n\ge1]\approx \lambda_n \approx Cq^n
]

So the decay is essentially exponential.

When (\lambda_n \ll 1):

[
1-e^{-\lambda_n}\approx \lambda_n
]

hence

[
E[X_n\mid X_n\ge1]\approx 1
]

So once nonsense (n)-grams become extremely rare, conditioning on “appeared at least once” forces the expectation toward 1. That is important: **the conditional curve bottoms out near 1, not 0**.

---

## Expected gradient from 1 to 5

Using

[
f(n)=\frac{Cq^n}{1-e^{-Cq^n}}
]

the discrete gradient is

[
g_n=f(n+1)-f(n)
===============

## \frac{Cq^{n+1}}{1-e^{-Cq^{n+1}}}

\frac{Cq^n}{1-e^{-Cq^n}}
]

for (n=1,2,3,4).

If you want a continuous approximation,

[
f'(n)
=====

\ln(q),\lambda_n
\cdot
\frac{1-e^{-\lambda_n}-\lambda_n e^{-\lambda_n}}
{(1-e^{-\lambda_n})^2}
]

with (\lambda_n=Cq^n).

Since (q<1), (\ln(q)<0), so the gradient is negative.

---

## Practical simplified formula for (n=1,\dots,5)

A good working model is piecewise:

[
E[X_n\mid X_n\ge1]
\approx
\max(1,;A\rho^{n-1})
]

so the gradient is approximately

[
\Delta_n \approx
\begin{cases}
A\rho^{n-1}(\rho-1), & A\rho^{n-1}\gg 1[4pt]
0, & A\rho^{n-1}\approx 1
\end{cases}
]

This captures the behavior nicely:

* early (n): exponential decline
* later (n): flattening near 1 because of the (X_n\ge1) condition

---

## Intuition for 1→5

For nonsense sequences:

* (n=1): many odd tokens can still occur globally
* (n=2): many disappear, survivors still repeat
* (n=3): repetitions collapse
* (n=4): mostly one-offs
* (n=5): almost every observed survivor has count very close to 1

So the expected gradient is usually:

* strongly negative at small (n)
* less negative as (n) grows
* approaching 0 from below near 5 if sparsity is severe

---

## Compact final form

If you want one neat formula to use, I’d use:

[
\boxed{
E[X_n\mid X_n\ge1]
\approx
\frac{Cq^n}{1-e^{-Cq^n}}
}
]

and

[
\boxed{
g_n
\approx
\frac{Cq^{n+1}}{1-e^{-Cq^{n+1}}}
--------------------------------

\frac{Cq^n}{1-e^{-Cq^n}}
}
]

This is better than plain (A\rho^{n-1}) because it naturally explains why the conditioned expectation stops falling below 1.

I can also plug in example values of (C) and (q) and show what the 1→5 curve looks like numerically.

