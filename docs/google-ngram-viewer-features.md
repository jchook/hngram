Google Ngram Viewer  [ Books Ngram Viewer ](https://books.google.com/ngrams)


## Advanced Usage

A few features of the Ngram Viewer may appeal to users who want to dig a little deeper into phrase usage: *wildcard search* , *inflection search* , *case insensitive search* , *part-of-speech tags* and *ngram compositions*.


#### Wildcard search

When you put a `*` in place of a word, the Ngram Viewer will display the top ten substitutions. For instance, to find the most popular words following "University of", search for " [University of \*.](https://books.google.com/ngrams/graph?content=University+of+*&year_start=1800&year_end=2019&corpus=en&smoothing=3)"

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00050% 0.00100% 0.00150% 0.00200% Ngrams chart for University of California,University of Chicago,University of Michigan,University of Illinois,University of Pennsylvania,University of Wisconsin,University of Minnesota,University of New,University of Texas,University of North  University of CaliforniaUniversity of ChicagoUniversity of MichiganUniversity of IllinoisUniversity of PennsylvaniaUniversity of WisconsinUniversity of MinnesotaUniversity of NewUniversity of TexasUniversity of North

You can right click on any of the replacement ngrams to collapse them all into the original wildcard query, with the result being the yearwise sum of the replacements. A subsequent right click expands the wildcard query back to all the replacements. Note that the Ngram Viewer only supports one \* per ngram.

Note that the top ten replacements are computed for the specified time range. You might therefore get different replacements for different year ranges. We've filtered punctuation symbols from the top ten list, but for words that often start or end sentences, you might see one of the sentence boundary symbols (`_START_` or `_END_`) as one of the replacements.


#### Inflection search

An *inflection* is the modification of a word to represent various grammatical categories such as aspect, case, gender, mood, number, person, tense and voice. You can search for them by appending \_INF to an ngram. For instance, searching " [book`_INF` a hotel](https://books.google.com/ngrams/graph?content=book_INF+a+hotel&year_start=1800&year_end=2022&corpus=en&smoothing=3)" will display results for "book", "booked", "books", and "booking":

  2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000000% 0.00000200% Ngrams chart for book a hotel,booked a hotel,booking a hotel,books a hotel  book a hotelbooked a hotelbooking a hotelbooks a hotel

Right clicking any inflection collapses all forms into their sum. Note that the Ngram Viewer only supports one `_INF` keyword per query.

**Warning:** You can't freely mix wildcard searches, inflections and case-insensitive searches for one particular ngram. However, you can search with either of these features for separate ngrams in a query: "book`_INF` a hotel, book \* hotel" is fine, but "book`_INF` \* hotel" is not.


#### Case insensitive search

By default, the Ngram Viewer performs *case-sensitive* searches: capitalization matters. You can perform a case-insensitive search by selecting the "case-insensitive" checkbox to the right of the query box. The Ngram Viewer will then display the yearwise sum of the most common case-insensitive variants of the input query. Here are two case-insensitive ngrams, ["Fitzgerald" and "Dupont"](https://books.google.com/ngrams/graph?content=Fitzgerald%2CDupont&year_start=1800&year_end=2022&corpus=en&smoothing=3&case_insensitive=true):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.000000% 0.000500% Ngrams chart for Fitzgerald (All),Dupont (All)  Fitzgerald (All)Dupont (All)

Right clicking any yearwise sum results in an expansion into the most common case-insensitive variants. For example, a right click on "Dupont (All)" results in the following four variants: "DuPont", "Dupont", "duPont" and "DUPONT".


#### Part-of-speech Tags

Consider the word *tackle* , which can be a verb ("tackle the problem") or a noun ("fishing tackle"). You can distinguish between these different forms by appending `_VERB` or `_NOUN` , example: " [tackle`_VERB` , tackle`_NOUN`](https://books.google.com/ngrams/graph?content=tackle_VERB%2 Ctackle_NOUN&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  1800 1900 2000(click on line/label for focus)  0.000000% 0.000500% Ngrams chart for tackle\_VERB,tackle\_NOUN  tackle\_VERBtackle\_NOUN

The full list of tags is as follows:

| `_NOUN_` | Noun | These tags can either stand alone (`_PRON_`) or can be appended to a word (she\_`PRON`) |
| `_PROPN_` | Proper Noun |
| `_VERB_` | Verb |
| `_ADJ_` | Adjective |
| `_ADV_` | Adverb |
| `_PRON_` | Pronoun |
| `_DET_` | Determiner or article |
| `_ADP_` | An adposition: either a preposition or a postposition |
| `_NUM_` | Numeral |
| `_CONJ_` | Conjunction |
| `_PRT_` | Particle |
| `_ROOT_` | Root of the parse tree | These tags must stand alone (e.g., `_START_` ) |
| `_START_` | Start of a sentence |
| `_END_` | End of a sentence |

Since the part-of-speech tags needn't attach to particular words, you can use the `DET` tag to search for *read a book* , *read the book* , *read that book* , *read this book* , and so on as follows, " [read `_DET_` book](https://books.google.com/ngrams/graph?content=read+_DET_+book&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

  1800 2000(click on line/label for focus)  0.000000% 0.000200% Ngrams chart for read \_DET\_ book  read \_DET\_ book

If you wanted to know what the most common determiners in this context are, you could combine wildcards and part-of-speech tags to [*read* `*_DET` *book*](https://books.google.com/ngrams/graph?content=read+*_DET+book&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.0000000% 0.0000200% 0.0000400% 0.0000600% 0.0000800% Ngrams chart for read the\_DET book,read a\_DET book,read this\_DET book,read that\_DET book,read any\_DET book,read every\_DET book,read no\_DET book,read some\_DET book,read another\_DET book,read The\_DET book  read the\_DET bookread a\_DET bookread this\_DET bookread that\_DET bookread any\_DET bookread every\_DET bookread no\_DET bookread some\_DET bookread another\_DET bookread The\_DET book

To get all the different inflections of the word *book* which have been followed by a `NOUN` in the corpus you can issue the query [*book* `_INF _NOUN_`](https://books.google.com/ngrams/graph?content=book_INF+_NOUN_&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00200% Ngrams chart for book \_NOUN\_,books \_NOUN\_,booking \_NOUN\_,booked \_NOUN\_  book \_NOUN\_books \_NOUN\_booking \_NOUN\_booked \_NOUN\_

Most frequent part-of-speech tags for a word can be retrieved with the wildcard functionality. Consider the query [*cooks\_\**](https://books.google.com/ngrams/graph?content=cook_*&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1900 1950 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% Ngrams chart for cook\_NOUN,cook\_VERB  cook\_NOUNcook\_VERB

The inflection keyword can also be combined with part-of-speech tags. For example, consider the query [*cook* `_INF` , *cook* `_VERB_` `INF`](https://books.google.com/ngrams/graph?content=cook_INF%2C+cook_VERB_INF&year_start=1800&year_end=2022&corpus=en&smoothing=3) below, that separates out the inflections of the verbal sense of "cook":

  1900 1950 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00100% 0.00200% Ngrams chart for cooking,cook,cooked,cooks,cooked\_VERB,cook\_VERB,cooking\_VERB,cooks\_VERB  cookingcookcookedcookscooked\_VERBcook\_VERBcooking\_VERBcooks\_VERB

The Ngram Viewer tags sentence boundaries, allowing you to identify ngrams at starts and ends of sentences with the `_START_` and `_END_` tags, example: " [`_START_` President Truman,`_START_` President Lincoln,`_START_` President Harding](https://books.google.com/ngrams/graph?content=_START_+President+Truman%2C+_START_+President+Lincoln%2C+_START_+President+Harding&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  2000(click on line/label for focus)  0.000000% 0.000100% Ngrams chart for \_START\_ President Truman,\_START\_ President Lincoln,\_START\_ President Harding  \_START\_ President Truman\_START\_ President Lincoln\_START\_ President Harding

Sometimes it helps to think about words in terms of dependencies rather than patterns. Let's say you want to know how often *tasty* modifies *dessert* . That is, you want to tally mentions of *tasty frozen dessert* , *crunchy, tasty dessert* , *tasty yet expensive dessert* , and all the other instances in which the word *tasty* is applied to *dessert* . For that, the Ngram Viewer provides dependency relations with the `=>` operator [dessert`=>`tasty](https://books.google.com/ngrams/graph?content=dessert%3D%3 Etasty&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1900 2000(click on line/label for focus)  0.000000000% 0.000000500% Ngrams chart for dessert=>tasty  dessert=>tasty

Every parsed sentence has a `_ROOT_` . Unlike other tags, `_ROOT_` doesn't stand for a particular word or position in the sentence. It's the root of the parse tree constructed by analyzing the syntax; you can think of it as a placeholder for what the main verb of the sentence is modifying. So here's how to identify how often *will* was the main verb of a sentence, " [`_ROOT_` `=>`will"](https://books.google.com/ngrams/graph?content=_ROOT_%3D%3 Ewill&year_start=1800&year_end=20122&corpus=en&smoothing=3):

  1800 1900 2000(click on line/label for focus)  0.00000% Ngrams chart for \_ROOT\_=>will  \_ROOT\_=>will

The above graph would include the sentence *Larry will decide.* but not *Larry said that he will decide* , since *will* isn't the main verb of that sentence.

Dependencies can be combined with wildcards. For example, consider the query [*drink=>\** `_NOUN`](https://books.google.com/ngrams/graph?content=drink%3D%3E*_NOUN&year_start=1800&year_end=2022&corpus=en&smoothing=3) below:

  1900 2000(click on line/label for focus, right click to expand/contract wildcards)  0.0000000% 0.0000200% 0.0000400% 0.0000600% 0.0000800% 0.0001000% Ngrams chart for drink=>water\_NOUN,drink=>wine\_NOUN,drink=>milk\_NOUN,drink=>tea\_NOUN,drink=>beer\_NOUN,drink=>coffee\_NOUN,drink=>cup\_NOUN,drink=>blood\_NOUN,drink=>glass\_NOUN,drink=>health\_NOUN  drink=>water\_NOUNdrink=>wine\_NOUNdrink=>milk\_NOUNdrink=>tea\_NOUNdrink=>beer\_NOUNdrink=>coffee\_NOUNdrink=>cup\_NOUNdrink=>blood\_NOUNdrink=>glass\_NOUNdrink=>health\_NOUN

"Pure" part-of-speech tags can be mixed freely with regular words in 1-, 2-, 3-, 4-, and 5-grams (e.g., `the _ADJ_ toast` or `_DET_ _ADJ_ toast` ). " [`_DET_` bright`_ADJ` rainbow](https://books.google.com/ngrams/graph?content=_DET_+bright_ADJ+rainbow&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  1800 2000(click on line/label for focus)  0.00000000% 0.00000200% Ngrams chart for \_DET\_ bright\_ADJ rainbow  \_DET\_ bright\_ADJ rainbow


#### Ngram Compositions

The Ngram Viewer provides five operators that you can use to combine ngrams: `+` , `-` , `/` , `*` , and `:`.

| `+` | Sums the expressions on either side, letting you combine multiple ngram time series into one. |
| `-` | Subtracts the expression on the right from the expression on the left, giving you a way to measure one ngram relative to another. Because users often want to search for hyphenated phrases, put spaces on either side of the `-` sign. |
| `/` | Divides the expression on the left by the expression on the right, which is useful for isolating the behavior of an ngram with respect to another. |
| `*` | Multiplies the expression on the left by the number on the right, making it easier to compare ngrams of very different frequencies. (Be sure to enclose the entire ngram in parentheses so that \* isn't interpreted as a wildcard.) |
| `:` | Applies the ngram on the left to the corpus on the right, allowing you to compare ngrams across different corpora. |

The Ngram Viewer will try to guess whether to apply these behaviors. You can use parentheses to force them on, and square brackets to force them off. Example: `and/or` will divide *and* by *or* ; to measure the usage of the phrase *and/or* , use `[and/or]` . And `well-meaning` will search for the phrase *well-meaning* ; if you want to subtract meaning from well, use `(well - meaning)`.

To demonstrate the `+` operator, here's how you might find the sum of *game* , *sport* , and *play* , [game, sport ,play, (game + sport + play)](https://books.google.com/ngrams/graph?content=game%2C+sport%2C+play%2C+%28game+%2B+sport+%2B+play%29&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus)  0.0000% 0.0200% Ngrams chart for game,sport,play,(game + sport + play)  gamesportplay(game + sport + play)

When determining whether people wrote more about choices over the years, you could compare *choice* , *selection* , *option* , and *alternative* , specifying the noun forms to avoid the adjective forms (e.g., *choice delicacy* , *alternative music* ), example " [(choice`_NOUN` + selection`_NOUN` + option`_NOUN` + alternative`_NOUN`)](https://books.google.com/ngrams/graph?content=%28choice_NOUN+%2B+selection_NOUN+%2B+option_NOUN+%2B+alternative_NOUN%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0000% 0.0200% Ngrams chart for (choice\_NOUN + selection\_NOUN + option\_NOUN + alternative\_NOUN)  (choice\_NOUN + selection\_NOUN + option\_NOUN + alternative\_NOUN)

Ngram subtraction gives you an easy way to compare one set of ngrams to another, " [((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))](https://books.google.com/ngrams/graph?content=%28choice_NOUN+%2B+selection_NOUN+%2B+option_NOUN+%2B+alternative_NOUN%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0000000% Ngrams chart for ((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))  ((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))

Here's how you might combine `+` and `/` to show how the word *applesauce* has blossomed at the expense of *apple sauce* , " [(applesauce / (apple sauce + applesauce))](https://books.google.com/ngrams/graph?content=%28applesauce+%2F+%28apple+sauce+%2B+applesauce%29%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0% 100.0% Ngrams chart for (applesauce / (apple sauce + applesauce))  (applesauce / (apple sauce + applesauce))

The `*` operator is useful when you want to compare ngrams of widely varying frequencies, like *violin* and the more esoteric *theremin* , " [(theremin \* 1000),violin"](https://books.google.com/ngrams/graph?content=%28theremin+*+1000%29%2C+violin&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  2000(click on line/label for focus)  0.00000% Ngrams chart for (theremin \* 1000),violin  (theremin \* 1000)violin

